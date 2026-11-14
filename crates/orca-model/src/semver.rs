// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries semver code: semver_parse, semver_parse_version, semver_is_valid and their helpers
// (contains, has_valid_chars, has_valid_length, parse_slice) of deps_src/semver/semver.c, as
// OrcaSlicer v2.4.2 bundles it (with BambuStudio's fourth number), translated on 2026-11-10.
// Copyright (c) 2015-2017 Tomas Aparicio, under the MIT licence (licenses/MIT-semver.txt).
//
// Carries OrcaSlicer code: Semver's constructors and Semver::parse of src/libslic3r/Semver.hpp, of
// OrcaSlicer v2.4.2, translated on 2026-11-10. Copyright the OrcaSlicer authors, the BambuStudio
// authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General
// Public License, version 3 (AGPL-3.0; LICENSE).

//! `Semver::parse`: a version string read by the bundled C library `semver.c`, which the 3MF
//! reader calls on the `Application` and `OrcaSlicer` metadata (`bbs_3mf.cpp:3946-3962`).
//!
//! The string may hold only digits, ASCII letters, `.`, `-` and `+`, and at most 255 bytes. The
//! metadata is what follows the first `+`, the prerelease what follows the first `-` before it;
//! the rest is up to four numbers split at `.`, each of at most 50 bytes and read with `strtol`
//! (32-bit `long`, so a number past its range is `LONG_MAX`), which must end at the `.` or at the
//! end. Major and minor are needed, patch is not; a fourth number makes the patch
//! `patch * 100 + fourth` (BambuStudio's `AA.BB.CC.DD`), wrapping as MSVC's `int` does. A fifth
//! fails. The comparisons, `semver_render` and the helpers nothing here reaches (`strcut`,
//! `binary_comparison`, `parse_int`) are not translated.

/// `semver.c`'s `MAX_SIZE`: the longest string it reads.
const MAX_SIZE: usize = 255;
/// `SLICE_SIZE`: the longest number it reads.
const SLICE_SIZE: usize = 50;
/// `VALID_CHARS`: `NUMBERS ALPHA DELIMITERS`.
const VALID_CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ.-+";

/// `Semver` (`semver_t`): `major.minor.patch`, with the prerelease and the metadata when given.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Semver {
    /// `major`.
    pub major: i32,
    /// `minor`.
    pub minor: i32,
    /// `patch`.
    pub patch: i32,
    /// `metadata`: what follows the first `+`.
    pub metadata: Option<String>,
    /// `prerelease`: what follows the first `-` before the metadata.
    pub prerelease: Option<String>,
}

/// `contains(c, matrix, len)`.
fn contains(c: u8, matrix: &[u8]) -> bool {
    matrix.contains(&c)
}

/// `has_valid_chars(str, matrix)`.
fn has_valid_chars(s: &[u8], matrix: &[u8]) -> bool {
    s.iter().all(|&c| contains(c, matrix))
}

/// `has_valid_length(s)`.
fn has_valid_length(s: &[u8]) -> bool {
    s.len() <= MAX_SIZE
}

/// `semver_is_valid(s)`.
fn semver_is_valid(s: &[u8]) -> bool {
    has_valid_length(s) && has_valid_chars(s, VALID_CHARS)
}

/// `parse_slice(buf, sep)`: what follows the first `sep` in `buf`, `buf` cut there.
fn parse_slice(buf: &mut Vec<u8>, sep: u8) -> Option<String> {
    let pr = buf.iter().position(|&c| c == sep)?;
    let part = String::from_utf8_lossy(&buf[pr + 1..]).into_owned();
    buf.truncate(pr);
    Some(part)
}

/// `semver_parse_version(str, ver)`: the numbers of `s`, into `ver`; whether they were valid.
fn semver_parse_version(s: &[u8], ver: &mut Semver) -> bool {
    let mut slice = Some(0usize);
    let mut index = 0;
    // non mandatory
    ver.patch = 0;
    while let Some(start) = slice {
        let more = index < 4;
        index += 1;
        if !more {
            break;
        }
        let next = s[start..]
            .iter()
            .position(|&c| c == b'.')
            .map(|i| start + i);
        let len = match next {
            None => s.len() - start,
            Some(n) => n - start,
        };
        if len > SLICE_SIZE {
            return false;
        }
        // Cast to integer and store
        let r = orca_fmt::parse::strtol(&s[start..], 10);
        let value = r.value;
        let endptr = start + r.end;
        if Some(endptr) != next && endptr < s.len() {
            return false;
        }
        match index {
            1 => ver.major = value,
            2 => ver.minor = value,
            3 => ver.patch = value,
            // BBS: add convert for AA.BB.CC.DD
            4 => ver.patch = ver.patch.wrapping_mul(100).wrapping_add(value),
            _ => {}
        }
        // Continue with the next slice
        slice = next.map(|n| n + 1);
    }
    // Major and minor versions are mandatory, patch version is not mandatory.
    index == 2 || index == 3 || index == 4
}

/// `semver_parse(str, ver)`: whether `s` is a version; `ver` holds what was read either way.
fn semver_parse(s: &[u8], ver: &mut Semver) -> bool {
    if !semver_is_valid(s) {
        return false;
    }
    let mut buf = s.to_vec();
    ver.metadata = parse_slice(&mut buf, b'+');
    ver.prerelease = parse_slice(&mut buf, b'-');
    semver_parse_version(&buf, ver)
}

impl Semver {
    /// `Semver::parse(str)`: the version, or none when `semver_parse` refuses it.
    #[must_use]
    pub fn parse(s: &str) -> Option<Semver> {
        let mut ver = Semver::default();
        semver_parse(s.as_bytes(), &mut ver).then_some(ver)
    }

    /// `maj()`.
    #[must_use]
    pub fn maj(&self) -> i32 {
        self.major
    }

    /// `min()`.
    #[must_use]
    pub fn min(&self) -> i32 {
        self.minor
    }

    /// `patch()`.
    #[must_use]
    pub fn patch(&self) -> i32 {
        self.patch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(major: i32, minor: i32, patch: i32) -> Semver {
        Semver {
            major,
            minor,
            patch,
            ..Semver::default()
        }
    }

    // Expected values read from semver.c: semver_parse (:141-162), semver_parse_version
    // (:174-215), parse_slice (:108-128), semver_is_valid (:565-568, MAX_SIZE :22). Not
    // replayed: the records that rest on it (m_is_bbl_3mf) are pieces 2's and 3's.
    #[test]
    fn versions() {
        assert_eq!(Semver::parse("2.3"), Some(v(2, 3, 0)));
        assert_eq!(Semver::parse("2.3.1"), Some(v(2, 3, 1)));
        // :202 BambuStudio's AA.BB.CC.DD.
        assert_eq!(Semver::parse("01.10.02.73"), Some(v(1, 10, 273)));
        // :185 the loop ends after four slices with index 5.
        assert_eq!(Semver::parse("1.2.3.4.5"), None);
        assert_eq!(Semver::parse("7"), None);
        assert_eq!(Semver::parse(""), None);
        // :195 a slice must end at the dot or the end.
        assert_eq!(Semver::parse("1.2x"), None);
        assert_eq!(Semver::parse("1..2"), Some(v(1, 0, 2)));
        // :191 a slice of more than 50 bytes.
        assert_eq!(Semver::parse(&format!("1.{}", "0".repeat(51))), None);
        assert_eq!(
            Semver::parse(&format!("1.{}", "0".repeat(50))),
            Some(v(1, 0, 0))
        );
        // strtol's 32-bit long: past its range, LONG_MAX.
        assert_eq!(Semver::parse("1.99999999999"), Some(v(1, i32::MAX, 0)));
        // Characters and length (semver_is_valid).
        assert_eq!(Semver::parse("2.3 "), None);
        assert_eq!(Semver::parse(&format!("1.2-{}", "a".repeat(252))), None);
        assert_eq!(
            Semver::parse(&format!("1.2-{}", "a".repeat(251))),
            Some(Semver {
                prerelease: Some("a".repeat(251)),
                ..v(1, 2, 0)
            })
        );
    }

    #[test]
    fn prerelease_and_metadata() {
        assert_eq!(
            Semver::parse("2.3.0-beta+build-7"),
            Some(Semver {
                prerelease: Some("beta".to_owned()),
                metadata: Some("build-7".to_owned()),
                ..v(2, 3, 0)
            })
        );
        assert_eq!(
            Semver::parse("2.3-rc-1"),
            Some(Semver {
                prerelease: Some("rc-1".to_owned()),
                ..v(2, 3, 0)
            })
        );
    }
}
