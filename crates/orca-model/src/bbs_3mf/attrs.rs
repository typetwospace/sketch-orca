// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: ZipUnicodePathExtraField (lines 70-102), hex_wrap (456-472), the
// attribute getters (481-520) and the transform, offset, unit and object type helpers (593-676) of
// src/libslic3r/Format/bbs_3mf.cpp, of OrcaSlicer v2.4.2, translated on 2026-11-10. Copyright the
// OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa
// Research), under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! The 3MF reader's small readers: an XML attribute by name as a string, `float`, `int` or `bool`,
//! a 3MF transform and offset, the model's unit, the object types it takes, and the Info-ZIP
//! Unicode path extra field.
//!
//! The getters search the attributes a start tag specified (`XML_GetSpecifiedAttributeCount`,
//! twice their number), names and values alternating, and take the first of the name. A `float`
//! is read by the bundled fast_float ([`super::fast_float`]), an `int` by Spirit's `qi::int_`
//! ([`super::spirit`]), both 0 when nothing is read; a `bool` by `atoi`, true when the attribute
//! is missing. A transform is 12 numbers and an offset 3, split at runs of spaces (a space at an
//! end adds an empty number) and read with `atof`; any other count gives the identity or zero.
//! `version_error` (lines 474-479), which only the version checks throw, is not needed: a load
//! under `LoadModel | AddDefaultInstances` makes no such check.

use orca_eigen::{Transform3d, Vec3d};

use super::names::{BBS_VALID_OBJECT_TYPES, BBS_VALID_OBJECT_TYPES_COUNT};
use crate::transformation::split_compress_spaces;

/// `ZipUnicodePathExtraField::encode(u8path, path)`: the Info-ZIP Unicode path extra field (id
/// 0x7075, version 1, the CRC-32 of `path`, then `u8path`) when the two differ, else nothing.
#[must_use]
pub fn zip_unicode_path_extra_field_encode(u8path: &[u8], path: &[u8]) -> Vec<u8> {
    let mut extra = Vec::new();
    if u8path != path {
        // 0x7075 - for Unicode filenames
        extra.push(0x75);
        extra.push(0x70);
        let len = (5 + u8path.len()) as u16;
        extra.push((len & 0xff) as u8);
        extra.push((len >> 8) as u8);
        let crc = orca_zip::crc::mz_crc32(0, path);
        extra.push(0x01); // version 1
        extra.extend_from_slice(&crc.to_le_bytes()); // Little Endian
        extra.extend_from_slice(u8path);
    }
    extra
}

/// `ZipUnicodePathExtraField::decode(extra)` with the default `path` (empty): the Unicode path
/// of the first such field that fits in `extra`, else `decode_path("")`, which is empty.
///
/// The field's length is read from two `char`s, which MSVC signs: a byte of 0x80 or more widens
/// to a negative value before it is turned into `uint16_t`, so its high byte is 0xFF.
#[must_use]
pub fn zip_unicode_path_extra_field_decode(extra: &[u8]) -> Vec<u8> {
    let e = extra.len();
    let mut p = 0usize;
    while p + 4 < e {
        let b2 = i32::from(extra[p + 2] as i8) as u16;
        let b3 = i32::from(extra[p + 3] as i8) as u16;
        let len = (i32::from(b2) | (i32::from(b3) << 8)) as u16;
        let len = usize::from(len);
        if extra[p] == 0x75
            && extra[p + 1] == 0x70
            && len >= 5
            && p + 4 + len < e
            && extra[p + 4] == 0x01
        {
            return extra[p + 9..p + 4 + len].to_vec();
        }
        p += 4 + len;
    }
    Vec::new()
}

/// `operator<<(ostream, hex_wrap<T>{t})`: `t` in lower-case hexadecimal, padded with zeros to
/// twice its size in bytes (`size`), as an unsigned value of that size.
#[must_use]
pub fn hex_wrap(t: u64, size: usize) -> String {
    format!("{t:0width$x}", width = size * 2)
}

/// `bbs_get_attribute_value_charptr(attributes, attributes_size, attribute_key)`.
#[must_use]
pub fn get_attribute_value_charptr<'a>(
    attributes: &[&'a [u8]],
    attributes_size: u32,
    attribute_key: &str,
) -> Option<&'a [u8]> {
    if attributes_size == 0 || !attributes_size.is_multiple_of(2) {
        return None;
    }
    let mut a = 0usize;
    while a < attributes_size as usize {
        if *attributes.get(a)? == attribute_key.as_bytes() {
            return attributes.get(a + 1).copied();
        }
        a += 2;
    }
    None
}

/// `bbs_get_attribute_value_string(...)`: the value, or empty.
#[must_use]
pub fn get_attribute_value_string(
    attributes: &[&[u8]],
    attributes_size: u32,
    attribute_key: &str,
) -> String {
    match get_attribute_value_charptr(attributes, attributes_size, attribute_key) {
        // expat's strings are UTF-8.
        Some(text) => String::from_utf8_lossy(text).into_owned(),
        None => String::new(),
    }
}

/// `bbs_get_attribute_value_float(...)`: `fast_float::from_chars` of the value, 0 when it reads
/// nothing or the attribute is missing.
#[must_use]
pub fn get_attribute_value_float(
    attributes: &[&[u8]],
    attributes_size: u32,
    attribute_key: &str,
) -> f32 {
    let mut value = 0.0f32;
    if let Some(text) = get_attribute_value_charptr(attributes, attributes_size, attribute_key) {
        super::fast_float::from_chars(text, &mut value);
    }
    value
}

/// `bbs_get_attribute_value_int(...)`: `qi::parse(..., qi::int_, value)` of the value, 0 when it
/// reads nothing or the attribute is missing, the digits before an overflow on one.
#[must_use]
pub fn get_attribute_value_int(
    attributes: &[&[u8]],
    attributes_size: u32,
    attribute_key: &str,
) -> i32 {
    let mut value = 0i32;
    if let Some(text) = get_attribute_value_charptr(attributes, attributes_size, attribute_key) {
        super::spirit::parse_int(text, &mut value);
    }
    value
}

/// `bbs_get_attribute_value_bool(...)`: `atoi` of the value is not 0; true when it is missing.
#[must_use]
pub fn get_attribute_value_bool(
    attributes: &[&[u8]],
    attributes_size: u32,
    attribute_key: &str,
) -> bool {
    match get_attribute_value_charptr(attributes, attributes_size, attribute_key) {
        Some(text) => orca_fmt::parse::atoi(text) != 0,
        None => true,
    }
}

/// `bbs_get_transform_from_3mf_specs_string(mat_str)`: 12 numbers, stored 4x3 and transposed
/// into the matrix's first three rows; the identity for an empty string or another count.
#[must_use]
pub fn get_transform_from_3mf_specs_string(mat_str: &str) -> Transform3d {
    let mut ret = Transform3d::identity();
    if mat_str.is_empty() {
        // empty string means default identity matrix
        return ret;
    }
    let mat_elements_str = split_compress_spaces(mat_str);
    if mat_elements_str.len() != 12 {
        // invalid data, return identity matrix
        return ret;
    }
    let mut i = 0;
    // matrices are stored into 3mf files as 4x3
    // we need to transpose them
    for c in 0..4 {
        for r in 0..3 {
            ret.m
                .set(r, c, orca_fmt::parse::atof(mat_elements_str[i].as_bytes()));
            i += 1;
        }
    }
    ret
}

/// `bbs_get_offset_from_3mf_specs_string(vec_str)`: 3 numbers; zero for an empty string or
/// another count.
#[must_use]
pub fn get_offset_from_3mf_specs_string(vec_str: &str) -> Vec3d {
    if vec_str.is_empty() {
        // empty string means default zero offset
        return Vec3d::zero();
    }
    let vec_elements_str = split_compress_spaces(vec_str);
    if vec_elements_str.len() != 3 {
        // invalid data, return zero offset
        return Vec3d::zero();
    }
    let a = |i: usize| orca_fmt::parse::atof(vec_elements_str[i].as_bytes());
    Vec3d::new(a(0), a(1), a(2))
}

/// `bbs_get_unit_factor(unit)`: millimetres per unit, as a `float`.
#[must_use]
pub fn get_unit_factor(unit: &str) -> f32 {
    match unit {
        "micron" => 0.001,
        "centimeter" => 10.0,
        "inch" => 25.4,
        "foot" => 304.8,
        "meter" => 1000.0,
        // default "millimeters" (see specification)
        _ => 1.0,
    }
}

/// `bbs_is_valid_object_type(type)`: empty (a model, by the specification), `model` or `other`.
#[must_use]
pub fn is_valid_object_type(ty: &str) -> bool {
    // if the type is empty defaults to "model" (see specification)
    if ty.is_empty() {
        return true;
    }
    BBS_VALID_OBJECT_TYPES[..BBS_VALID_OBJECT_TYPES_COUNT as usize].contains(&ty)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ATTS: [&[u8]; 8] = [b"id", b"7x", b"name", b"a&b", b"x", b"-1.5e1", b"id", b"9"];

    // Expected values read from bbs_3mf.cpp:481-520: the first value of the name among the
    // specified attributes; an odd or zero count finds nothing; int through qi::int_, float
    // through fast_float, bool through atoi (true when missing). Not replayed: piece 1's
    // records refuse before a getter decides what they hold.
    #[test]
    fn getters() {
        assert_eq!(get_attribute_value_int(&ATTS, 8, "id"), 7);
        assert_eq!(get_attribute_value_int(&ATTS, 3, "id"), 0);
        assert_eq!(get_attribute_value_int(&ATTS, 0, "id"), 0);
        assert_eq!(get_attribute_value_string(&ATTS, 8, "name"), "a&b");
        assert_eq!(get_attribute_value_string(&ATTS, 2, "name"), "");
        assert_eq!(get_attribute_value_float(&ATTS, 8, "x"), -15.0);
        assert_eq!(get_attribute_value_float(&ATTS, 8, "y"), 0.0);
        assert!(get_attribute_value_bool(&ATTS, 8, "printable"));
        assert!(get_attribute_value_bool(&ATTS, 8, "id"));
        assert!(!get_attribute_value_bool(&[b"p", b"0"], 2, "p"));
        assert!(!get_attribute_value_bool(&[b"p", b"x"], 2, "p"));
    }

    // Expected values read from bbs_3mf.cpp:593-620: 12 numbers, column by column into rows 0-2.
    #[test]
    fn transforms_and_offsets() {
        let t = get_transform_from_3mf_specs_string("1 2 3 4 5 6 7 8 9 10 11 12");
        assert_eq!(t.m.at(0, 0), 1.0);
        assert_eq!(t.m.at(1, 0), 2.0);
        assert_eq!(t.m.at(2, 0), 3.0);
        assert_eq!(t.m.at(0, 1), 4.0);
        assert_eq!(t.m.at(2, 3), 12.0);
        assert_eq!(t.m.at(3, 3), 1.0);
        assert_eq!(t.m.at(3, 0), 0.0);
        // A leading space adds an empty first number: 13, so the identity.
        let id = Transform3d::identity();
        let lead = get_transform_from_3mf_specs_string(" 1 2 3 4 5 6 7 8 9 10 11 12");
        assert_eq!(lead.m.at(0, 0), id.m.at(0, 0));
        assert_eq!(lead.m.at(1, 0), id.m.at(1, 0));
        // A doubled space counts once.
        let doubled = get_transform_from_3mf_specs_string("1  0 0 0 1 0 0 0 1 5 6 7");
        assert_eq!(doubled.m.at(0, 3), 5.0);
        assert_eq!(doubled.m.at(2, 3), 7.0);
        assert_eq!(
            get_offset_from_3mf_specs_string("1 -2 0x10"),
            Vec3d::new(1.0, -2.0, 16.0)
        );
        assert_eq!(get_offset_from_3mf_specs_string("1 2"), Vec3d::zero());
    }

    // Expected values read from bbs_3mf.cpp:646-676.
    #[test]
    fn units_and_types() {
        assert_eq!(get_unit_factor("micron"), 0.001f32);
        assert_eq!(get_unit_factor("inch"), 25.4f32);
        assert_eq!(get_unit_factor("millimeter"), 1.0);
        assert_eq!(get_unit_factor("Inch"), 1.0);
        assert!(is_valid_object_type(""));
        assert!(is_valid_object_type("model"));
        assert!(is_valid_object_type("other"));
        assert!(!is_valid_object_type("support"));
    }

    // Expected values read from bbs_3mf.cpp:70-102.
    #[test]
    fn unicode_path_extra_field() {
        let extra = zip_unicode_path_extra_field_encode("é.model".as_bytes(), b"e.model");
        assert_eq!(&extra[..2], &[0x75, 0x70]);
        assert_eq!(extra[4], 1);
        // decode needs one byte past the field (p + 4 + len < e).
        let mut padded = extra.clone();
        assert_eq!(zip_unicode_path_extra_field_decode(&padded), b"");
        padded.push(0);
        assert_eq!(
            zip_unicode_path_extra_field_decode(&padded),
            "é.model".as_bytes()
        );
        assert!(zip_unicode_path_extra_field_encode(b"a", b"a").is_empty());
        // Another field first is skipped by its length.
        let mut two = vec![0x01, 0x00, 0x02, 0x00, 0xAA, 0xBB];
        two.extend_from_slice(&padded);
        assert_eq!(
            zip_unicode_path_extra_field_decode(&two),
            "é.model".as_bytes()
        );
        assert_eq!(hex_wrap(0x2a, 4), "0000002a");
    }
}
