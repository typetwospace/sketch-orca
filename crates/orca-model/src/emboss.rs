// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: FontProp, EmbossStyle and TextConfiguration of
// src/libslic3r/TextConfiguration.hpp, and EmbossProjection, HealedExPolygons and EmbossShape with
// its SvgFile of src/libslic3r/EmbossShape.hpp, of OrcaSlicer v2.4.2, translated on 2026-11-11.
// Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer
// authors (Prusa Research), under the GNU Affero General Public License, version 3 (AGPL-3.0;
// LICENSE).

//! A part's text and the shape it was embossed from, as the 3MF reader reads them from
//! `model_settings.config` (`<slic3rpe:text>` and `<slic3rpe:shape>`), for the part to carry.
//! Only the data: the GUI draws and edits them, and the command line slices the part's mesh.
//!
//! What the reader never fills is left out: an `EmbossShape`'s shapes and the final shape's
//! polygons (`shapes_with_ids`, `final_shape.expolygons`: always empty from a 3MF file) and an svg
//! file's parsed image (`image`, a cache the GUI fills).

use std::sync::Arc;

use orca_eigen::Transform3d;

/// `FontProp::HorizontalAlign`, an `enum class` over `int`: the 3MF reader casts a one-digit
/// index to it unchecked (`static_cast`), so any `int` can be held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HorizontalAlign(pub i32);

impl HorizontalAlign {
    /// `left`.
    pub const LEFT: Self = Self(0);
    /// `center`.
    pub const CENTER: Self = Self(1);
    /// `right`.
    pub const RIGHT: Self = Self(2);
}

/// `FontProp::VerticalAlign`, as [`HorizontalAlign`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerticalAlign(pub i32);

impl VerticalAlign {
    /// `top`.
    pub const TOP: Self = Self(0);
    /// `center`.
    pub const CENTER: Self = Self(1);
    /// `bottom`.
    pub const BOTTOM: Self = Self(2);
}

/// `FontProp`: how the text is set.
#[derive(Clone, Debug, PartialEq)]
pub struct FontProp {
    /// `char_gap` [in font point].
    pub char_gap: Option<i32>,
    /// `line_gap` [in font point].
    pub line_gap: Option<i32>,
    /// `boldness` [in mm].
    pub boldness: Option<f32>,
    /// `skew` [ration x:y].
    pub skew: Option<f32>,
    /// `collection_number`.
    pub collection_number: Option<u32>,
    /// `per_glyph`.
    pub per_glyph: bool,
    /// `align`.
    pub align: (HorizontalAlign, VerticalAlign),
    /// `size_in_mm` [in mm].
    pub size_in_mm: f32,
    /// `family`.
    pub family: Option<String>,
    /// `face_name`.
    pub face_name: Option<String>,
    /// `style`.
    pub style: Option<String>,
    /// `weight`.
    pub weight: Option<String>,
}

impl Default for FontProp {
    /// `FontProp(float line_height = 10.f)`.
    fn default() -> Self {
        FontProp {
            char_gap: None,
            line_gap: None,
            boldness: None,
            skew: None,
            collection_number: None,
            per_glyph: false,
            align: (HorizontalAlign::CENTER, VerticalAlign::CENTER),
            size_in_mm: 10.0,
            family: None,
            face_name: None,
            style: None,
            weight: None,
        }
    }
}

/// `EmbossStyle::Type`: how the font is described.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EmbossStyleType {
    /// `undefined` (0).
    #[default]
    Undefined,
    /// `wx_win_font_descr`: on Windows.
    WxWinFontDescr,
    /// `wx_lin_font_descr`: on Linux.
    WxLinFontDescr,
    /// `wx_mac_font_descr`: on Mac OS.
    WxMacFontDescr,
    /// `file_path`.
    FilePath,
}

/// `EmbossStyle`: a named font and its properties.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EmbossStyle {
    /// `name`.
    pub name: String,
    /// `path`: the font's descriptor.
    pub path: String,
    /// `type`.
    pub ty: EmbossStyleType,
    /// `prop`.
    pub prop: FontProp,
}

/// `TextConfiguration`: a text part's style and text.
#[derive(Clone, Debug, PartialEq)]
pub struct TextConfiguration {
    /// `style`.
    pub style: EmbossStyle,
    /// `text`.
    pub text: String,
}

impl Default for TextConfiguration {
    fn default() -> Self {
        TextConfiguration {
            style: EmbossStyle::default(),
            text: "None".to_owned(),
        }
    }
}

/// `EmbossProjection`: how the shape is embossed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmbossProjection {
    /// `depth` [in local mm]: the size in the local Z direction.
    pub depth: f64,
    /// `use_surface`: the result is cut from the surface of the source objects.
    pub use_surface: bool,
}

impl Default for EmbossProjection {
    fn default() -> Self {
        EmbossProjection {
            depth: 1.0,
            use_surface: false,
        }
    }
}

/// A `std::string` the 3MF reader made at the size an archive entry claims and extracted the
/// entry into: the bytes extraction wrote, then NULs up to the claim. The NULs are counted, not
/// allocated, so a claim the data cannot fill allocates nothing (`parity/HAZARDS.md`, the 3MF
/// table).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClaimedString {
    /// The bytes written.
    pub bytes: Vec<u8>,
    /// The NULs after them.
    pub nuls: u64,
}

impl ClaimedString {
    /// The string's length.
    #[must_use]
    pub fn len(&self) -> u64 {
        self.bytes.len() as u64 + self.nuls
    }

    /// Whether the string is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The text as the readers that split, parse or stream it see it: the bytes, then one NUL for
    /// any run of them. Every reader stops at, or carries along, the first NUL the same way
    /// however many follow it (`std::string` operations treat it as a character, `c_str()`,
    /// rapidxml and expat end there).
    #[must_use]
    pub fn text(&self) -> Vec<u8> {
        let mut t = self.bytes.clone();
        if self.nuls > 0 {
            t.push(0);
        }
        t
    }
}

/// `EmbossShape::SvgFile`: the svg file an embossed shape came from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SvgFile {
    /// `path`: the file on the local computer; empty when it cannot be reloaded from disk.
    pub path: String,
    /// `path_in_3mf`: the file in the 3MF archive; empty when it is not stored there yet.
    pub path_in_3mf: String,
    /// `file_data`: the loaded file, shared with the reader's map of the archive's svg files.
    pub file_data: Option<Arc<ClaimedString>>,
}

/// `EmbossShape`: a part embossed from a shape, its scale, projection and source.
#[derive(Clone, Debug, PartialEq)]
pub struct EmbossShape {
    /// `final_shape.is_healed`: whether the union of the shapes is healed; `None` where the C++
    /// leaves it uninitialized (`TextConfigurationSerialization::read_old`'s default-initialized
    /// `EmbossShape`).
    pub final_shape_is_healed: Option<bool>,
    /// `scale`: the multiplier from the shape's integers to millimetres.
    pub scale: f64,
    /// `projection`.
    pub projection: EmbossProjection,
    /// `fix_3mf_tr`: the correction of the part's transformation stored with the 3MF file.
    pub fix_3mf_tr: Option<Transform3d>,
    /// `svg_file`.
    pub svg_file: Option<SvgFile>,
}

impl Default for EmbossShape {
    /// `EmbossShape es;`: `scale` is `SCALING_FACTOR`.
    fn default() -> Self {
        EmbossShape {
            final_shape_is_healed: None,
            scale: orca_geom::point::SCALING_FACTOR,
            projection: EmbossProjection::default(),
            fix_3mf_tr: None,
            svg_file: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values read from TextConfiguration.hpp (FontProp(10.f), the centre alignment,
    // text "None") and EmbossShape.hpp (depth 1, scale SCALING_FACTOR).
    #[test]
    fn defaults() {
        let t = TextConfiguration::default();
        assert_eq!(t.text, "None");
        assert!((t.style.prop.size_in_mm - 10.0).abs() < f32::EPSILON);
        assert_eq!(
            t.style.prop.align,
            (HorizontalAlign::CENTER, VerticalAlign::CENTER)
        );
        let e = EmbossShape::default();
        assert!((e.projection.depth - 1.0).abs() < f64::EPSILON);
        assert!((e.scale - 0.000_001).abs() < f64::EPSILON);
        let c = ClaimedString {
            bytes: b"ab".to_vec(),
            nuls: 3,
        };
        assert_eq!(c.len(), 5);
        assert_eq!(c.text(), b"ab\0");
    }
}
