// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Boost code: path_algorithms::filename_v3, extension_v3, replace_extension_v3, append_v3,
// append_separator_if_needed, path::make_preferred, find_root_directory_start, find_filename_size
// and is_root_separator of libs/filesystem/src/path.cpp, of Boost 1.84.0, the Boost OrcaSlicer
// v2.4.2's dependency build pins, translated on 2026-11-14 (orca-o3-layer-3); their copyright
// holders are Beman Dawes, Andrey Semashev and the Boost.Filesystem contributors, under the Boost
// Software License 1.0 (licenses/BSL-1.0.txt).

//! What PrintBase.cpp takes from `boost::filesystem::path` (version 3, Boost 1.84's default):
//! [`filename`], [`extension`], [`replace_extension`], [`append`] (`operator/`) and
//! [`make_preferred`], with Windows' separators (`/` and `\`) and root names (`c:`, `//net`,
//! `\\?\`) on Windows.
//!
//! The C++ path holds UTF-16 on Windows, converted from and back to the narrow text by the path's
//! code conversion; the separators, the colon and the dot it looks for are ASCII, so working on
//! the UTF-8 bytes finds the same places, and an ASCII name comes back unchanged.
//!
//! orca-model's `utils` translates `filename_v3` and its helpers for `path_stem`, privately; these
//! belong there (Boost.Filesystem), and are repeated here until orca-model makes `filename` public.

/// Boost's `is_directory_separator`: `/`, and `\` on Windows.
fn is_separator(c: u8) -> bool {
    c == b'/' || (cfg!(windows) && c == b'\\')
}

/// `path::preferred_separator`.
const PREFERRED_SEPARATOR: u8 = if cfg!(windows) { b'\\' } else { b'/' };

/// `find_separator`: the first separator at or after `from`, or the end.
fn find_separator(p: &[u8], from: usize) -> usize {
    p[from..]
        .iter()
        .position(|&c| is_separator(c))
        .map_or(p.len(), |i| from + i)
}

/// `find_root_directory_start(path, size, root_name_size)`: where the root directory starts (the
/// path's length when there is none), and the root name's length.
fn find_root_directory_start(p: &[u8]) -> (usize, usize) {
    let size = p.len();
    if size == 0 {
        return (0, 0);
    }
    let mut parsing_root_name = false;
    let mut pos = 0usize;
    if is_separator(p[0]) {
        if size >= 2 && is_separator(p[1]) {
            if size == 2 {
                return (2, 2);
            } else if cfg!(windows)
                && size >= 4
                && (p[2] == b'?' || p[2] == b'.')
                && is_separator(p[3])
            {
                // "\\?\" or "\\.\"
                parsing_root_name = true;
                pos += 4;
            } else if is_separator(p[2]) {
                // "///" is the root directory
                return (0, 0);
            } else {
                // "//net {/}"
                let end = find_separator(p, 2);
                return (end, end);
            }
        } else if cfg!(windows) && size >= 4 && p[1] == b'?' && p[2] == b'?' && is_separator(p[3]) {
            // "\??\"
            parsing_root_name = true;
            pos += 4;
        } else {
            return (0, 0);
        }
    }
    let mut root_name_size = 0usize;
    if cfg!(windows) && size - pos >= 2 && p[pos].is_ascii_alphabetic() {
        // "c:" or "prn:"
        let mut i = pos + 1;
        while i < size && (p[i].is_ascii_alphanumeric() || p[i] == b'$') {
            i += 1;
        }
        if i < size && p[i] == b':' {
            pos = i + 1;
            root_name_size = pos;
            parsing_root_name = false;
            if pos < size && is_separator(p[pos]) {
                return (pos, root_name_size);
            }
        }
    }
    if !parsing_root_name {
        return (size, root_name_size);
    }
    let end = find_separator(p, pos);
    (end, end)
}

/// `is_root_separator(str, root_dir_pos, pos)`: whether the separator at `pos` is the root
/// directory's (duplicates after it included).
fn is_root_separator(p: &[u8], root_dir_pos: usize, mut pos: usize) -> bool {
    while pos > root_dir_pos && is_separator(p[pos - 1]) {
        pos -= 1;
    }
    pos == root_dir_pos
}

/// `find_filename_size(str, root_name_size, end_pos)`.
fn find_filename_size(p: &[u8], root_name_size: usize, end_pos: usize) -> usize {
    let mut pos = end_pos;
    while pos > root_name_size {
        pos -= 1;
        if is_separator(p[pos]) {
            pos += 1;
            break;
        }
    }
    end_pos - pos
}

/// `path(p).filename()` (`filename_v3`): the last element; the root directory or the root name
/// when that is all there is; `.` after a trailing separator.
#[must_use]
pub fn filename(path: &str) -> &str {
    let p = path.as_bytes();
    let size = p.len();
    let (root_dir_pos, root_name_size) = find_root_directory_start(p);
    let (pos, filename_size);
    if root_dir_pos < size
        && is_separator(p[size - 1])
        && is_root_separator(p, root_dir_pos, size - 1)
    {
        // The root directory.
        pos = root_dir_pos;
        filename_size = 1;
    } else if root_name_size == size {
        // The root name.
        pos = 0;
        filename_size = root_name_size;
    } else {
        filename_size = find_filename_size(p, root_name_size, size);
        pos = size - filename_size;
        if filename_size == 0
            && pos > root_name_size
            && is_separator(p[pos - 1])
            && !is_root_separator(p, root_dir_pos, pos - 1)
        {
            return ".";
        }
    }
    &path[pos..pos + filename_size]
}

/// `path(p).extension()` (`extension_v3`): the file name from its last dot (a name that starts
/// with its only dot is all extension); none for `.`, `..` and a name without a dot.
#[must_use]
pub fn extension(path: &str) -> &str {
    let name = filename(path);
    if name == "." || name == ".." {
        return "";
    }
    match name.rfind('.') {
        Some(pos) => &name[pos..],
        None => "",
    }
}

/// `path.replace_extension(new_extension)` (`replace_extension_v3`): the extension, with its
/// dot, removed; then `new_extension` appended, with a dot before it when it has none.
pub fn replace_extension(path: &mut String, new_extension: &str) {
    // Erase the existing extension, including the dot, if any.
    let ext_pos = path.len() - extension(path).len();
    path.truncate(ext_pos);
    if !new_extension.is_empty() {
        // Append new_extension, adding the dot if necessary.
        if !new_extension.starts_with('.') {
            path.push('.');
        }
        path.push_str(new_extension);
    }
}

/// `append_separator_if_needed(p)`: the preferred separator after a path that does not end with
/// one (or, on Windows, with a colon).
fn append_separator_if_needed(path: &mut String) {
    if let Some(&last) = path.as_bytes().last()
        && !(cfg!(windows) && last == b':')
        && !is_separator(last)
    {
        path.push(char::from(PREFERRED_SEPARATOR));
    }
}

/// `path(p) / that` (`append_v3`): `that` after `p`, with a separator between them when `that`
/// does not start with one; nothing for an empty `that`.
#[must_use]
pub fn append(p: &str, that: &str) -> String {
    let mut out = p.to_string();
    if let Some(&first) = that.as_bytes().first() {
        if !is_separator(first) {
            append_separator_if_needed(&mut out);
        }
        out.push_str(that);
    }
    out
}

/// `path.make_preferred()`: on Windows each `/` made `\`; elsewhere unchanged.
#[must_use]
pub fn make_preferred(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_and_extensions() {
        assert_eq!(filename("C:/models/part.v2.stl"), "part.v2.stl");
        assert_eq!(filename("x.3mf"), "x.3mf");
        assert_eq!(filename(""), "");
        assert_eq!(filename("a/b/"), ".");
        assert_eq!(filename("/"), "/");
        assert_eq!(extension("a/b.c.gcode"), ".gcode");
        assert_eq!(extension(".hidden"), ".hidden");
        assert_eq!(extension("name."), ".");
        assert_eq!(extension("a/.."), "");
        assert_eq!(extension("noext"), "");
        if cfg!(windows) {
            assert_eq!(filename("c:"), "c:");
            assert_eq!(filename("c:\\dir\\f.txt"), "f.txt");
        }
    }

    #[test]
    fn replacing_and_appending() {
        let mut p = "out/part".to_string();
        replace_extension(&mut p, ".gcode");
        assert_eq!(p, "out/part.gcode");
        let mut q = "a.b".to_string();
        replace_extension(&mut q, "gcode");
        assert_eq!(q, "a.gcode");
        assert_eq!(
            append("dir", "f.gcode"),
            format!("dir{}f.gcode", char::from(PREFERRED_SEPARATOR))
        );
        assert_eq!(append("dir/", "f"), "dir/f");
        assert_eq!(append("dir", ""), "dir");
        assert_eq!(append("", "f"), "f");
    }
}
