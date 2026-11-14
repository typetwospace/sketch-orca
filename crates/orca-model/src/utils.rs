// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: set_temporary_dir, temporary_dir, encode_path, get_current_pid,
// xml_unescape and save_string_file of src/libslic3r/utils.cpp, of OrcaSlicer v2.4.2, translated on
// 2026-11-10. Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the
// PrusaSlicer authors (Prusa Research), under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).
//
// Carries Boost code: path_stem follows path::stem_v3, filename_v3, find_root_directory_start,
// find_filename_size and is_root_separator of libs/filesystem/src/path.cpp, and quoted follows
// operator<< of boost::filesystem::path (boost/filesystem/path.hpp) with boost::io::quoted
// (boost/io/quoted.hpp), of Boost 1.84.0, the Boost OrcaSlicer v2.4.2's dependency build pins,
// translated on 2026-11-10; their copyright holders are Beman Dawes, Andrey Semashev and the
// Boost.Filesystem contributors, and Beman Dawes and Glen Fernandes (boost/io/quoted.hpp), under
// the Boost Software License 1.0 (licenses/BSL-1.0.txt).

//! What the 3MF reader and `Model::get_backup_path` take from libslic3r's `utils.cpp` and
//! Boost.Filesystem.
//!
//! - [`set_temporary_dir`] and [`temporary_dir`]: the process's temporary folder, where the backup
//!   folders go (`cli/run.cpp` sets it to the job's work folder, the probe to a folder of its own;
//!   empty until set, as the C++ global is).
//! - [`encode_path`]: UTF-8 to the ANSI code page (`WideCharToMultiByte(CP_ACP)` through UTF-16,
//!   as `boost::nowide::widen` gives it), the bytes unchanged elsewhere.
//! - [`xml_unescape`]: `&lt;`, `&gt;` and `&amp;` in one pass.
//! - [`path_stem`]: `boost::filesystem::path(p).stem().string()` with Windows' separators (`/` and
//!   `\`) and root names. The C++ path holds UTF-16 on Windows; the separators, the colon and the
//!   dot it looks for are ASCII, so working on the UTF-8 bytes finds the same places.
//! - [`quoted`]: a path written to a stream (`"` around it, `&` before each `"` and `&`).
//! - [`local_time_now`] and [`put_time`]: `std::localtime(&std::time(0))` (the UCRT's
//!   `_time64` and `_localtime64_s` on Windows, the C library's `time` and `localtime_r`
//!   elsewhere) and `std::put_time` in the C locale for the fields the backup folder uses.
//! - [`save_string_file`]: Orca's own, a file written whole, failing as its stream does.

use std::sync::Mutex;

static TEMPORARY_DIR: Mutex<String> = Mutex::new(String::new());

/// `set_temporary_dir(dir)`.
pub fn set_temporary_dir(dir: &str) {
    let mut d = TEMPORARY_DIR
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *d = dir.to_owned();
}

/// `temporary_dir()`.
#[must_use]
pub fn temporary_dir() -> String {
    TEMPORARY_DIR
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// `get_current_pid()`: `GetCurrentProcessId()` on Windows, `getpid()` elsewhere.
#[must_use]
pub fn get_current_pid() -> u32 {
    std::process::id()
}

#[cfg(windows)]
mod sys {
    /// `struct tm`.
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    pub struct Tm {
        pub tm_sec: i32,
        pub tm_min: i32,
        pub tm_hour: i32,
        pub tm_mday: i32,
        pub tm_mon: i32,
        pub tm_year: i32,
        pub tm_wday: i32,
        pub tm_yday: i32,
        pub tm_isdst: i32,
    }

    // The static UCRT's own time functions (the engine links libucrt, .cargo/config.toml).
    unsafe extern "C" {
        pub fn _time64(t: *mut i64) -> i64;
        pub fn _localtime64_s(tm: *mut Tm, t: *const i64) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn WideCharToMultiByte(
            code_page: u32,
            flags: u32,
            wide: *const u16,
            wide_len: i32,
            multi: *mut u8,
            multi_len: i32,
            default_char: *const u8,
            used_default_char: *mut i32,
        ) -> i32;
    }

    /// `std::localtime(&std::time(0))`.
    pub fn local_time_now() -> Tm {
        let mut tm = Tm::default();
        // SAFETY: _time64 accepts a null pointer; _localtime64_s writes one struct tm.
        unsafe {
            let t = _time64(std::ptr::null_mut());
            _localtime64_s(&raw mut tm, &raw const t);
        }
        tm
    }
}

#[cfg(not(windows))]
mod sys {
    /// `struct tm`, with the fields glibc and the BSDs add.
    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub struct Tm {
        pub tm_sec: i32,
        pub tm_min: i32,
        pub tm_hour: i32,
        pub tm_mday: i32,
        pub tm_mon: i32,
        pub tm_year: i32,
        pub tm_wday: i32,
        pub tm_yday: i32,
        pub tm_isdst: i32,
        pub tm_gmtoff: std::ffi::c_long,
        pub tm_zone: *const std::ffi::c_char,
    }

    unsafe extern "C" {
        fn time(t: *mut i64) -> i64;
        fn localtime_r(t: *const i64, tm: *mut Tm) -> *mut Tm;
    }

    /// `std::localtime(&std::time(0))`.
    pub fn local_time_now() -> Tm {
        let mut tm = Tm {
            tm_sec: 0,
            tm_min: 0,
            tm_hour: 0,
            tm_mday: 0,
            tm_mon: 0,
            tm_year: 0,
            tm_wday: 0,
            tm_yday: 0,
            tm_isdst: 0,
            tm_gmtoff: 0,
            tm_zone: std::ptr::null(),
        };
        // SAFETY: time accepts a null pointer; localtime_r writes one struct tm.
        unsafe {
            let t = time(std::ptr::null_mut());
            localtime_r(&raw const t, &raw mut tm);
        }
        tm
    }
}

pub use sys::Tm;

/// `std::localtime(&std::time(0))`: the local time now.
#[must_use]
pub fn local_time_now() -> Tm {
    sys::local_time_now()
}

/// `std::put_time(tm, fmt)` in the C locale, for `%a`, `%b`, `%d`, `%H`, `%M`, `%S` and `%%`;
/// other characters as they are.
#[must_use]
pub fn put_time(tm: &Tm, fmt: &str) -> String {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut out = String::new();
    let mut chars = fmt.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('a') => out.push_str(DAYS[usize::try_from(tm.tm_wday).unwrap_or(0) % 7]),
            Some('b') => out.push_str(MONTHS[usize::try_from(tm.tm_mon).unwrap_or(0) % 12]),
            Some('d') => out.push_str(&format!("{:02}", tm.tm_mday)),
            Some('H') => out.push_str(&format!("{:02}", tm.tm_hour)),
            Some('M') => out.push_str(&format!("{:02}", tm.tm_min)),
            Some('S') => out.push_str(&format!("{:02}", tm.tm_sec)),
            Some('%') => out.push('%'),
            Some(o) => {
                out.push('%');
                out.push(o);
            }
            None => out.push('%'),
        }
    }
    out
}

/// `encode_path(src)`: a UTF-8 string in the local code page (`CP_ACP`) on Windows; the bytes
/// unchanged elsewhere.
#[must_use]
pub fn encode_path(src: &str) -> Vec<u8> {
    #[cfg(windows)]
    {
        // Convert the source utf8 encoded string to a wide string.
        let wide: Vec<u16> = src.encode_utf16().collect();
        if wide.is_empty() {
            return Vec::new();
        }
        let wide_len = wide.len() as i32;
        // Convert a wide string to a local code page.
        // SAFETY: the wide buffer holds wide_len units; the first call writes nothing, the second
        // at most size_needed bytes into a buffer of that size.
        let size_needed = unsafe {
            sys::WideCharToMultiByte(
                0,
                0,
                wide.as_ptr(),
                wide_len,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
                std::ptr::null_mut(),
            )
        };
        let mut str_dst = vec![0u8; usize::try_from(size_needed).unwrap_or(0)];
        // SAFETY: as above.
        unsafe {
            sys::WideCharToMultiByte(
                0,
                0,
                wide.as_ptr(),
                wide_len,
                str_dst.as_mut_ptr(),
                size_needed,
                std::ptr::null(),
                std::ptr::null_mut(),
            );
        }
        str_dst
    }
    #[cfg(not(windows))]
    {
        src.as_bytes().to_vec()
    }
}

/// `xml_unescape(s)`: `&lt;`, `&gt;` and `&amp;` replaced, left to right, in one pass.
#[must_use]
pub fn xml_unescape(s: &str) -> String {
    let b = s.as_bytes();
    let substr_is = |i: usize, pat: &[u8]| b[i..b.len().min(i + pat.len())] == *pat;
    let mut ret = String::new();
    let mut i = 0usize;
    let mut pos = 0usize;
    while i < b.len() {
        if b[i] == b'&' {
            if substr_is(i, b"&lt;") {
                ret.push_str(&s[pos..i]);
                ret.push('<');
                i += 4;
                pos = i;
            } else if substr_is(i, b"&gt;") {
                ret.push_str(&s[pos..i]);
                ret.push('>');
                i += 4;
                pos = i;
            } else if substr_is(i, b"&amp;") {
                ret.push_str(&s[pos..i]);
                ret.push('&');
                i += 5;
                pos = i;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    ret.push_str(&s[pos..]);
    ret
}

/// `boost::io::quoted(p.string(), '&')`, as a path is written to a stream: in `"`, with `&`
/// before each `"` and `&`.
#[must_use]
pub fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if c == '"' || c == '&' {
            out.push('&');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// Boost's `is_directory_separator`: `/`, and `\` on Windows.
fn is_separator(c: u8) -> bool {
    c == b'/' || (cfg!(windows) && c == b'\\')
}

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
                parsing_root_name = true;
                pos += 4;
            } else if is_separator(p[2]) {
                return (0, 0);
            } else {
                // "//net {/}"
                let end = find_separator(p, 2);
                return (end, end);
            }
        } else if cfg!(windows) && size >= 4 && p[1] == b'?' && p[2] == b'?' && is_separator(p[3]) {
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

/// `path::filename_v3()`.
fn path_filename(path: &str) -> &str {
    let p = path.as_bytes();
    let size = p.len();
    let (root_dir_pos, root_name_size) = find_root_directory_start(p);
    let (pos, filename_size);
    if root_dir_pos < size
        && is_separator(p[size - 1])
        && is_root_separator(p, root_dir_pos, size - 1)
    {
        // Return root directory
        pos = root_dir_pos;
        filename_size = 1;
    } else if root_name_size == size {
        // Return root name
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

/// `boost::filesystem::path(path).stem().string()` (`stem_v3`): the file name up to its last
/// dot, or all of it for `.` and `..`.
#[must_use]
pub fn path_stem(path: &str) -> String {
    let name = path_filename(path);
    if name != "."
        && name != ".."
        && let Some(pos) = name.rfind('.')
    {
        return name[..pos].to_owned();
    }
    name.to_owned()
}

/// `save_string_file(p, str)` (Orca's, `utils.cpp:1728-1734`): the file created or truncated and
/// written; a failure is the stream's `failbit` exception, MSVC's text.
///
/// # Errors
///
/// "ios_base::failbit set: iostream stream error" when the file cannot be opened or written.
pub fn save_string_file(p: &str, s: &str) -> Result<(), String> {
    std::fs::write(p, s.as_bytes())
        .map_err(|_| "ios_base::failbit set: iostream stream error".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values read from utils.cpp:1346-1380 (xml_unescape: one pass, three entities,
    // substr comparisons that stop at the end). Not replayed: piece 1's records refuse before
    // any metadata reaches it.
    #[test]
    fn unescape() {
        assert_eq!(xml_unescape("a &lt;b&gt; &amp; c"), "a <b> & c");
        assert_eq!(xml_unescape("&amp;amp;"), "&amp;");
        assert_eq!(xml_unescape("&quot;&#65;&"), "&quot;&#65;&");
        assert_eq!(xml_unescape("&lt"), "&lt");
        assert_eq!(xml_unescape("x&gt;"), "x>");
        assert_eq!(xml_unescape(""), "");
    }

    // Expected values read from Boost 1.84's path.cpp: stem_v3 (:709-719) cuts at the last dot of
    // filename_v3 (:671-699), which starts after the last separator or the root name.
    #[test]
    fn stems() {
        assert_eq!(path_stem("C:\\a\\b\\box.3mf"), "box");
        assert_eq!(path_stem("C:/a/b.c/box.x.3mf"), "box.x");
        assert_eq!(path_stem("box.3mf"), "box");
        assert_eq!(path_stem(".3mf"), "");
        assert_eq!(path_stem("noext"), "noext");
        assert_eq!(path_stem("a/.."), "..");
        if cfg!(windows) {
            assert_eq!(path_stem("C:box.3mf"), "box");
            assert_eq!(path_stem("C:\\a\\b\\"), ".");
        }
    }

    #[test]
    fn quoting_and_time() {
        assert_eq!(quoted("C:\\a&b\"c"), "\"C:\\a&&b&\"c\"");
        let tm = Tm {
            tm_sec: 5,
            tm_min: 4,
            tm_hour: 3,
            tm_mday: 9,
            tm_mon: 10,
            tm_wday: 1,
            ..local_time_now()
        };
        assert_eq!(put_time(&tm, "%a_%b_%d/%H_%M_%S#"), "Mon_Nov_09/03_04_05#");
    }

    #[test]
    fn ascii_paths_keep_their_bytes() {
        assert_eq!(encode_path("3D/3dmodel.model"), b"3D/3dmodel.model");
        assert_eq!(encode_path(""), b"");
    }
}
