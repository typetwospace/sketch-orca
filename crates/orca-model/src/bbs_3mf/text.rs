// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: TextConfigurationSerialization's reading side, its bimaps and
// bimap_cvt, read_horizontal_align and read_vertical_align, and read_emboss_shape, of
// src/libslic3r/Format/bbs_3mf.cpp (lines 4038-4071, 9115-9170, 9214-9320 and 9414-9447) of
// OrcaSlicer v2.4.2, translated on 2026-11-11. Copyright the OrcaSlicer authors, the BambuStudio
// authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General
// Public License, version 3 (AGPL-3.0; LICENSE).

//! A text part's `<slic3rpe:text>` and an embossed part's `<slic3rpe:shape>` in
//! `model_settings.config`, read from their attributes with the 3MF reader's getters.
//!
//! The bimaps name the font descriptor's type (`file_name`, `wxFontDescriptor_Windows`,
//! `_Linux`, `_MacOsX`) and the alignments (`left`, `center`, `right`; `top`, `middle`,
//! `bottom`); a name they do not hold gives the default (`undefined`, centre). An alignment of
//! one character is first tried as Spirit's `qi::int_` and, when it reads a digit, cast to the
//! alignment unchecked, so `7` is held as 7. Both readers always give a value, so the C++'s tests
//! for none never fail. `to_xml` and `get_name` belong to the writer, not translated.

use orca_eigen::Transform3d;

use super::attrs::{
    get_attribute_value_float, get_attribute_value_int, get_attribute_value_string,
    get_transform_from_3mf_specs_string,
};
use super::names::{
    BOLDNESS_ATTR, CHAR_GAP_ATTR, COLLECTION_NUMBER_ATTR, DEPTH_ATTR, FONT_DESCRIPTOR_ATTR,
    FONT_DESCRIPTOR_TYPE_ATTR, FONT_FACE_NAME_ATTR, FONT_FAMILY_ATTR, FONT_STYLE_ATTR,
    FONT_WEIGHT_ATTR, HORIZONTAL_ALIGN_ATTR, LINE_GAP_ATTR, LINE_HEIGHT_ATTR, PER_GLYPH_ATTR,
    SHAPE_SCALE_ATTR, SKEW_ATTR, STYLE_NAME_ATTR, SVG_FILE_PATH_ATTR, SVG_FILE_PATH_IN_3MF_ATTR,
    TEXT_DATA_ATTR, TRANSFORM_ATTR, UNHEALED_ATTR, USE_SURFACE_ATTR, VERTICAL_ALIGN_ATTR,
};
use super::spirit::parse_int;
use crate::emboss::{
    EmbossProjection, EmbossShape, EmbossStyle, EmbossStyleType, FontProp, HorizontalAlign,
    SvgFile, TextConfiguration, VerticalAlign,
};

/// `TextConfigurationSerialization::type_to_name`.
const TYPE_TO_NAME: [(EmbossStyleType, &str); 4] = [
    (EmbossStyleType::FilePath, "file_name"),
    (EmbossStyleType::WxWinFontDescr, "wxFontDescriptor_Windows"),
    (EmbossStyleType::WxLinFontDescr, "wxFontDescriptor_Linux"),
    (EmbossStyleType::WxMacFontDescr, "wxFontDescriptor_MacOsX"),
];

/// `TextConfigurationSerialization::horizontal_align_to_name`.
const HORIZONTAL_ALIGN_TO_NAME: [(HorizontalAlign, &str); 3] = [
    (HorizontalAlign::LEFT, "left"),
    (HorizontalAlign::CENTER, "center"),
    (HorizontalAlign::RIGHT, "right"),
];

/// `TextConfigurationSerialization::vertical_align_to_name`.
const VERTICAL_ALIGN_TO_NAME: [(VerticalAlign, &str); 3] = [
    (VerticalAlign::TOP, "top"),
    (VerticalAlign::CENTER, "middle"),
    (VerticalAlign::BOTTOM, "bottom"),
];

/// `bimap_cvt(bmap, s, def_value)`: the first of the pair whose second is `s` (the bimap's right
/// view), else the default.
fn bimap_cvt<F: Copy>(bmap: &[(F, &str)], s: &str, def_value: F) -> F {
    // only for back and forward compatibility
    bmap.iter()
        .find(|(_, name)| *name == s)
        .map_or(def_value, |(f, _)| *f)
}

/// `read_horizontal_align(attributes, num_attributes, horizontal_align_to_name)`.
fn read_horizontal_align(atts: &[&[u8]], n: u32) -> HorizontalAlign {
    let horizontal_align_str = get_attribute_value_string(atts, n, HORIZONTAL_ALIGN_ATTR);

    // Back compatibility
    // PS 2.6.0 do not have align
    if horizontal_align_str.is_empty() {
        return HorizontalAlign::CENTER;
    }

    // Back compatibility
    // PS 2.6.1 store indices(0|1|2) instead of text for align
    if horizontal_align_str.len() == 1 {
        let mut horizontal_align_int = 0;
        if parse_int(
            &horizontal_align_str.as_bytes()[..1],
            &mut horizontal_align_int,
        ) {
            return HorizontalAlign(horizontal_align_int);
        }
    }

    bimap_cvt(
        &HORIZONTAL_ALIGN_TO_NAME,
        &horizontal_align_str,
        HorizontalAlign::CENTER,
    )
}

/// `read_vertical_align(attributes, num_attributes, vertical_align_to_name)`.
fn read_vertical_align(atts: &[&[u8]], n: u32) -> VerticalAlign {
    let vertical_align_str = get_attribute_value_string(atts, n, VERTICAL_ALIGN_ATTR);

    // Back compatibility
    // PS 2.6.0 do not have align
    if vertical_align_str.is_empty() {
        return VerticalAlign::CENTER;
    }

    // Back compatibility
    // PS 2.6.1 store indices(0|1|2) instead of text for align
    if vertical_align_str.len() == 1 {
        let mut vertical_align_int = 0;
        if parse_int(&vertical_align_str.as_bytes()[..1], &mut vertical_align_int) {
            return VerticalAlign(vertical_align_int);
        }
    }

    bimap_cvt(
        &VERTICAL_ALIGN_TO_NAME,
        &vertical_align_str,
        VerticalAlign::CENTER,
    )
}

/// `TextConfigurationSerialization::read(attributes, num_attributes)`: always a value.
#[must_use]
pub fn read(atts: &[&[u8]], n: u32) -> Option<TextConfiguration> {
    let mut fp = FontProp::default();
    let char_gap = get_attribute_value_int(atts, n, CHAR_GAP_ATTR);
    if char_gap != 0 {
        fp.char_gap = Some(char_gap);
    }
    let line_gap = get_attribute_value_int(atts, n, LINE_GAP_ATTR);
    if line_gap != 0 {
        fp.line_gap = Some(line_gap);
    }
    let boldness = get_attribute_value_float(atts, n, BOLDNESS_ATTR);
    if boldness.abs() > f32::EPSILON {
        fp.boldness = Some(boldness);
    }
    let skew = get_attribute_value_float(atts, n, SKEW_ATTR);
    if skew.abs() > f32::EPSILON {
        fp.skew = Some(skew);
    }
    let per_glyph = get_attribute_value_int(atts, n, PER_GLYPH_ATTR);
    if per_glyph == 1 {
        fp.per_glyph = true;
    }

    fp.align = (read_horizontal_align(atts, n), read_vertical_align(atts, n));

    let collection_number = get_attribute_value_int(atts, n, COLLECTION_NUMBER_ATTR);
    if collection_number > 0 {
        fp.collection_number = Some(collection_number.unsigned_abs());
    }

    fp.size_in_mm = get_attribute_value_float(atts, n, LINE_HEIGHT_ATTR);

    let family = get_attribute_value_string(atts, n, FONT_FAMILY_ATTR);
    if !family.is_empty() {
        fp.family = Some(family);
    }
    let face_name = get_attribute_value_string(atts, n, FONT_FACE_NAME_ATTR);
    if !face_name.is_empty() {
        fp.face_name = Some(face_name);
    }
    let style = get_attribute_value_string(atts, n, FONT_STYLE_ATTR);
    if !style.is_empty() {
        fp.style = Some(style);
    }
    let weight = get_attribute_value_string(atts, n, FONT_WEIGHT_ATTR);
    if !weight.is_empty() {
        fp.weight = Some(weight);
    }

    let style_name = get_attribute_value_string(atts, n, STYLE_NAME_ATTR);
    let font_descriptor = get_attribute_value_string(atts, n, FONT_DESCRIPTOR_ATTR);
    let type_str = get_attribute_value_string(atts, n, FONT_DESCRIPTOR_TYPE_ATTR);
    let ty = bimap_cvt(&TYPE_TO_NAME, &type_str, EmbossStyleType::Undefined);

    let text = get_attribute_value_string(atts, n, TEXT_DATA_ATTR);
    let es = EmbossStyle {
        name: style_name,
        path: font_descriptor,
        ty,
        prop: fp,
    };
    Some(TextConfiguration { style: es, text })
}

/// `TextConfigurationSerialization::read_old(attributes, num_attributes)`: the shape of a 3MF
/// file written before shapes were, from the text's own attributes.
#[must_use]
pub fn read_old(atts: &[&[u8]], n: u32) -> EmbossShape {
    let mut es = EmbossShape::default();
    let fix_tr_mat_str = get_attribute_value_string(atts, n, TRANSFORM_ATTR);
    if !fix_tr_mat_str.is_empty() {
        es.fix_3mf_tr = Some(get_transform_from_3mf_specs_string(&fix_tr_mat_str));
    }

    if get_attribute_value_int(atts, n, USE_SURFACE_ATTR) == 1 {
        es.projection.use_surface = true;
    }

    es.projection.depth = f64::from(get_attribute_value_float(atts, n, DEPTH_ATTR));

    let use_surface = get_attribute_value_int(atts, n, USE_SURFACE_ATTR);
    if use_surface == 1 {
        es.projection.use_surface = true;
    }

    es
}

/// libslic3r's `is_approx(value, test_value)` with the default `EPSILON`.
fn is_approx(value: f64, test_value: f64) -> bool {
    (value - test_value).abs() < orca_geom::point::EPSILON
}

/// `read_emboss_shape(attributes, num_attributes)`: always a value.
#[must_use]
pub fn read_emboss_shape(atts: &[&[u8]], n: u32) -> Option<EmbossShape> {
    let scale = f64::from(get_attribute_value_float(atts, n, SHAPE_SCALE_ATTR));
    let unhealed = get_attribute_value_int(atts, n, UNHEALED_ATTR);
    let is_healed = unhealed != 1;

    let mut projection = EmbossProjection {
        depth: f64::from(get_attribute_value_float(atts, n, DEPTH_ATTR)),
        ..EmbossProjection::default()
    };
    if is_approx(projection.depth, 0.0) {
        projection.depth = 10.0;
    }

    let use_surface = get_attribute_value_int(atts, n, USE_SURFACE_ATTR);
    if use_surface == 1 {
        projection.use_surface = true;
    }

    let mut fix_tr_mat: Option<Transform3d> = None;
    let fix_tr_mat_str = get_attribute_value_string(atts, n, TRANSFORM_ATTR);
    if !fix_tr_mat_str.is_empty() {
        fix_tr_mat = Some(get_transform_from_3mf_specs_string(&fix_tr_mat_str));
    }

    let file_path = get_attribute_value_string(atts, n, SVG_FILE_PATH_ATTR);
    let file_path_3mf = get_attribute_value_string(atts, n, SVG_FILE_PATH_IN_3MF_ATTR);

    // MayBe: store also shapes to not store svg
    // But be carefull curve will be lost -> scale will not change sampling
    // shapes could be loaded from SVG
    // (the shapes and the final shape's polygons stay empty; crate::emboss leaves them out)
    let svg = SvgFile {
        path: file_path,
        path_in_3mf: file_path_3mf,
        file_data: None,
    };
    Some(EmbossShape {
        final_shape_is_healed: Some(is_healed),
        scale,
        projection,
        fix_3mf_tr: fix_tr_mat,
        svg_file: Some(svg),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atts<'a>(pairs: &[(&'a str, &'a str)]) -> Vec<&'a [u8]> {
        pairs
            .iter()
            .flat_map(|(k, v)| [k.as_bytes(), v.as_bytes()])
            .collect()
    }

    // Expected values read from bbs_3mf.cpp:9216-9299: defaults for what is missing, a one-digit
    // alignment cast unchecked, a name the bimap does not hold the centre, the type by its name.
    #[test]
    fn texts() {
        let a = atts(&[
            ("text", "hi"),
            ("style_name", "s"),
            ("font_descriptor", "Arial"),
            ("font_descriptor_type", "wxFontDescriptor_Windows"),
            ("char_gap", "3"),
            ("boldness", "0.0000001"),
            ("skew", "0.5"),
            ("per_glyph", "1"),
            ("horizontal", "7"),
            ("vertical", "bottom"),
            ("collection", "-2"),
            ("line_height", "4"),
            ("family", "f"),
        ]);
        let t = read(&a, a.len() as u32).expect("always a value");
        assert_eq!(t.text, "hi");
        assert_eq!(t.style.name, "s");
        assert_eq!(t.style.path, "Arial");
        assert_eq!(t.style.ty, EmbossStyleType::WxWinFontDescr);
        let p = &t.style.prop;
        assert_eq!(p.char_gap, Some(3));
        assert_eq!(p.line_gap, None);
        assert_eq!(p.boldness, None);
        assert_eq!(p.skew, Some(0.5));
        assert!(p.per_glyph);
        assert_eq!(p.align, (HorizontalAlign(7), VerticalAlign::BOTTOM));
        assert_eq!(p.collection_number, None);
        assert!((p.size_in_mm - 4.0).abs() < f32::EPSILON);
        assert_eq!(p.family.as_deref(), Some("f"));
        assert_eq!(p.face_name, None);

        let b = atts(&[("horizontal", "x"), ("vertical", "middle")]);
        let t = read(&b, b.len() as u32).expect("always a value");
        assert_eq!(
            t.style.prop.align,
            (HorizontalAlign::CENTER, VerticalAlign::CENTER)
        );
        assert_eq!(t.style.ty, EmbossStyleType::Undefined);
        assert!(t.style.prop.size_in_mm.abs() < f32::EPSILON);
        assert_eq!(t.text, "");
    }

    // Expected values read from bbs_3mf.cpp:9301-9319 and 9414-9447: a depth of about 0 is 10 for
    // a shape, kept for an old text; unhealed only when 1.
    #[test]
    fn shapes() {
        let a = atts(&[
            ("scale", "0.5"),
            ("depth", "0.00001"),
            ("use_surface", "1"),
            ("unhealed", "1"),
            ("filepath3mf", "3D/s.svg"),
        ]);
        let s = read_emboss_shape(&a, a.len() as u32).expect("always a value");
        assert!((s.scale - 0.5).abs() < f64::EPSILON);
        assert!((s.projection.depth - 10.0).abs() < f64::EPSILON);
        assert!(s.projection.use_surface);
        assert_eq!(s.final_shape_is_healed, Some(false));
        assert_eq!(s.fix_3mf_tr, None);
        let svg = s.svg_file.expect("always an svg file");
        assert_eq!(svg.path, "");
        assert_eq!(svg.path_in_3mf, "3D/s.svg");

        let old = read_old(&a, a.len() as u32);
        assert!((old.projection.depth - f64::from(0.00001f32)).abs() < f64::EPSILON);
        assert!(old.projection.use_surface);
        assert_eq!(old.final_shape_is_healed, None);
        assert!(old.svg_file.is_none());
        assert!((old.scale - 0.000_001).abs() < f64::EPSILON);
    }
}
