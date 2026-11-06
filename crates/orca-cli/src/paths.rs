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
// Carries Boost code: is_absolute_path follows find_root_directory_start and path::is_absolute of
// libs/filesystem/src/path.cpp and include/boost/filesystem/path.hpp, and same_file's last step
// follows equivalent and status of libs/filesystem/src/operations.cpp, of Boost 1.84.0, the Boost
// OrcaSlicer v2.4.2's dependency build pins, on 2026-11-06; their copyright holders are Beman Dawes,
// Andrey Semashev and the Boost.Filesystem contributors, under the Boost Software License 1.0
// (licenses/BSL-1.0.txt).

//! Paths as the C++ front end reads them (cli/common.cpp): [`is_absolute_path`] (Boost.Filesystem's
//! `is_absolute`), and [`same_file`], whether two paths may name one file, as Windows opens them
//! (`GetFullPathNameW`'s conversion, the `\\?\`-style prefixes and NTFS stream names dropped, the
//! longest existing part resolved by the file system, letter case ignored as NTFS ignores it), or
//! as two existing names of one file (a hard link).

/// Boost's `is_directory_separator` on Windows: `/` or `\`.
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

/// `is_absolute_path(path)`: not empty, and Boost.Filesystem's `is_absolute`: on Windows a root
/// name and a root directory (`C:\`, `\\server\share`, `\\?\C:\`), elsewhere a root directory.
#[must_use]
pub fn is_absolute_path(path: &str) -> bool {
    let p = path.as_bytes();
    if p.is_empty() {
        return false;
    }
    let (root_dir, root_name_size) = find_root_directory_start(p);
    let has_root_directory = root_dir < p.len();
    if cfg!(windows) {
        root_name_size > 0 && has_root_directory
    } else {
        has_root_directory
    }
}

#[cfg(windows)]
mod win {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION, GetLastError, HANDLE,
        INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_NAME_NORMALIZED,
        FILE_READ_ATTRIBUTES, FILE_READ_EA, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        GetFileAttributesW, GetFileInformationByHandle, GetFinalPathNameByHandleW,
        GetFullPathNameW, INVALID_FILE_ATTRIBUTES, OPEN_EXISTING, VOLUME_NAME_DOS,
    };

    /// A path as UTF-16, without its terminating zero.
    pub fn widen(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s).encode_wide().collect()
    }

    fn with_nul(s: &[u16]) -> Vec<u16> {
        let mut v = s.to_vec();
        v.push(0);
        v
    }

    fn len_i32(n: usize) -> i32 {
        i32::try_from(n).unwrap_or(i32::MAX)
    }

    /// `win32_full_path(p)`: `GetFullPathNameW`'s conversion of `p`, or `p` when it gives none.
    pub fn win32_full_path(p: &[u16]) -> Vec<u16> {
        let z = with_nul(p);
        // SAFETY: z is NUL-terminated; a null buffer of size 0 asks for the size.
        let mut size =
            unsafe { GetFullPathNameW(z.as_ptr(), 0, std::ptr::null_mut(), std::ptr::null_mut()) };
        let mut tries = 0;
        while size > 0 && tries < 4 {
            let mut out = vec![0u16; size as usize];
            // SAFETY: out holds `size` units.
            let got = unsafe {
                GetFullPathNameW(z.as_ptr(), size, out.as_mut_ptr(), std::ptr::null_mut())
            };
            if got == 0 {
                break;
            }
            if got < size {
                out.truncate(got as usize);
                return out;
            }
            size = got;
            tries += 1;
        }
        p.to_vec()
    }

    /// `starts_with_ci`: `CompareStringOrdinal` without letter case on the prefix's length.
    fn starts_with_ci(s: &[u16], prefix: &[u16]) -> bool {
        // SAFETY: both slices are valid for the lengths given.
        s.len() >= prefix.len()
            && unsafe {
                CompareStringOrdinal(
                    s.as_ptr(),
                    len_i32(prefix.len()),
                    prefix.as_ptr(),
                    len_i32(prefix.len()),
                    1,
                )
            } == CSTR_EQUAL
    }

    /// `without_device_prefix(p)`: `\\?\`, `\\.\` and `\??\` taken off (`\\?\UNC\server` read as
    /// `\\server`), the rest converted again, up to four rounds.
    pub fn without_device_prefix(mut p: Vec<u16>) -> Vec<u16> {
        let prefixes = [widen("\\\\?\\"), widen("\\\\.\\"), widen("\\??\\")];
        let unc = widen("UNC\\");
        for _ in 0..4 {
            let mut stripped = false;
            for prefix in &prefixes {
                if !starts_with_ci(&p, prefix) {
                    continue;
                }
                p = p[prefix.len()..].to_vec();
                if starts_with_ci(&p, &unc) {
                    let mut q = widen("\\\\");
                    q.extend_from_slice(&p[4..]);
                    p = q;
                }
                stripped = true;
                break;
            }
            if !stripped {
                break;
            }
            p = win32_full_path(&p);
        }
        p
    }

    /// `without_streams(p)`: a name's `:stream` parts dropped, the drive's colon kept, separators
    /// written as `\`.
    pub fn without_streams(p: &[u16]) -> Vec<u16> {
        let (colon, back, slash) = (u16::from(b':'), u16::from(b'\\'), u16::from(b'/'));
        let mut out = Vec::with_capacity(p.len());
        let mut start = 0;
        if p.len() >= 2 && p[1] == colon {
            out.extend_from_slice(&p[..2]);
            start = 2;
        }
        let mut in_stream = false;
        for &c in &p[start..] {
            if c == back || c == slash {
                in_stream = false;
                out.push(back);
            } else if c == colon {
                in_stream = true;
            } else if !in_stream {
                out.push(c);
            }
        }
        out
    }

    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the handle was opened here and is closed once.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// The file or folder `p` names, opened for `access` as Boost.Filesystem opens it, or the
    /// system's error code.
    fn open_or_error(p: &[u16], access: u32) -> Result<Handle, u32> {
        let z = with_nul(p);
        // SAFETY: z is NUL-terminated; no security attributes or template.
        let h = unsafe {
            CreateFileW(
                z.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null_mut(),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            // SAFETY: reads the calling thread's last error, right after the call that set it.
            Err(unsafe { GetLastError() })
        } else {
            Ok(Handle(h))
        }
    }

    fn open(p: &[u16], access: u32) -> Option<Handle> {
        open_or_error(p, access).ok()
    }

    /// `final_path(p)`: `GetFinalPathNameByHandleW` of the file or folder `p` opens, without its
    /// `\\?\` prefix; empty when it cannot be opened.
    pub fn final_path(p: &[u16]) -> Vec<u16> {
        let Some(h) = open(p, FILE_READ_ATTRIBUTES) else {
            return Vec::new();
        };
        let flags = FILE_NAME_NORMALIZED | VOLUME_NAME_DOS;
        // SAFETY: the handle is open; a null buffer of size 0 asks for the size.
        let mut size = unsafe { GetFinalPathNameByHandleW(h.0, std::ptr::null_mut(), 0, flags) };
        let mut out = Vec::new();
        let mut tries = 0;
        while size > 0 && tries < 4 {
            let mut buf = vec![0u16; size as usize];
            // SAFETY: buf holds `size` units.
            let got = unsafe { GetFinalPathNameByHandleW(h.0, buf.as_mut_ptr(), size, flags) };
            if got == 0 {
                break;
            }
            if got < size {
                buf.truncate(got as usize);
                out = buf;
                break;
            }
            size = got;
            tries += 1;
        }
        drop(h);
        if out.is_empty() {
            out
        } else {
            without_device_prefix(out)
        }
    }

    /// `win32_file_key(path)`: what the path names as Windows opens it, its longest existing part
    /// replaced by the file system's own path for it.
    pub fn win32_file_key(path: &str) -> Vec<u16> {
        let back = u16::from(b'\\');
        let mut p = without_streams(&without_device_prefix(win32_full_path(&widen(path))));
        while p.len() > 3 && p.last() == Some(&back) {
            p.pop();
        }
        let mut end = p.len();
        while end > 0 {
            let head = &p[..end];
            let probe = if head.len() == 2 && head[1] == u16::from(b':') {
                let mut h = head.to_vec();
                h.push(back);
                h
            } else {
                head.to_vec()
            };
            let real = final_path(&probe);
            if !real.is_empty() {
                let mut key = real;
                if end < p.len() {
                    if key.last() != Some(&back) {
                        key.push(back);
                    }
                    key.extend_from_slice(&p[end + 1..]);
                }
                return key;
            }
            match head.iter().rposition(|&c| c == back) {
                Some(slash) if slash >= 2 => end = slash,
                _ => break,
            }
        }
        p
    }

    /// `CompareStringOrdinal(a, b, ignore case) == CSTR_EQUAL`.
    pub fn equal_ignoring_case(a: &[u16], b: &[u16]) -> bool {
        // SAFETY: both slices are valid for the lengths given.
        unsafe {
            CompareStringOrdinal(
                a.as_ptr(),
                len_i32(a.len()),
                b.as_ptr(),
                len_i32(b.len()),
                1,
            ) == CSTR_EQUAL
        }
    }

    /// `boost::filesystem::exists(p, ec)` (`status_impl`), for files, folders and the links to
    /// them: the path opens; or it is refused (access denied) but `GetFileAttributesW` reads it
    /// and it is no reparse point; or another program holds it open (a sharing violation, which
    /// Boost reads as a file of unknown type).
    pub fn exists(path: &str) -> bool {
        let w = widen(path);
        match open_or_error(&w, FILE_READ_ATTRIBUTES | FILE_READ_EA) {
            Ok(_) => true,
            Err(ERROR_ACCESS_DENIED) => {
                let z = with_nul(&w);
                // SAFETY: z is NUL-terminated.
                let attrs = unsafe { GetFileAttributesW(z.as_ptr()) };
                if attrs == INVALID_FILE_ATTRIBUTES {
                    // SAFETY: reads the calling thread's last error, right after the call.
                    return unsafe { GetLastError() } == ERROR_SHARING_VIOLATION;
                }
                attrs & FILE_ATTRIBUTE_REPARSE_POINT == 0
            }
            Err(e) => e == ERROR_SHARING_VIOLATION,
        }
    }

    /// `boost::filesystem::is_regular_file(p, ec)`: the path opens (links followed) and is no
    /// folder, or it is refused (access denied) and `GetFileAttributesW` reads a file that is no
    /// folder or reparse point. A file another program holds open is of unknown type: not regular.
    pub fn is_regular_file(path: &str) -> bool {
        let w = widen(path);
        match open_or_error(&w, FILE_READ_ATTRIBUTES | FILE_READ_EA) {
            Ok(h) => {
                // SAFETY: zeroed is a valid BY_HANDLE_FILE_INFORMATION; the handle is open.
                let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
                // SAFETY: as above.
                let read = unsafe { GetFileInformationByHandle(h.0, &raw mut info) };
                read != 0 && info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
            }
            Err(ERROR_ACCESS_DENIED) => {
                let z = with_nul(&w);
                // SAFETY: z is NUL-terminated.
                let attrs = unsafe { GetFileAttributesW(z.as_ptr()) };
                attrs != INVALID_FILE_ATTRIBUTES
                    && attrs & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) == 0
            }
            Err(_) => false,
        }
    }

    /// `boost::filesystem::equivalent(a, b, ec)`: both open and are one file by volume, index,
    /// size and last write time; an error (neither opens) is not one file.
    pub fn equivalent(a: &str, b: &str) -> bool {
        let h2 = open(&widen(b), FILE_READ_ATTRIBUTES);
        let h1 = open(&widen(a), FILE_READ_ATTRIBUTES);
        let (Some(h1), Some(h2)) = (h1, h2) else {
            return false;
        };
        // SAFETY: zeroed is a valid BY_HANDLE_FILE_INFORMATION; the handles are open.
        let (mut i1, mut i2): (BY_HANDLE_FILE_INFORMATION, BY_HANDLE_FILE_INFORMATION) =
            unsafe { (std::mem::zeroed(), std::mem::zeroed()) };
        // SAFETY: as above.
        if unsafe { GetFileInformationByHandle(h1.0, &raw mut i1) } == 0
            || unsafe { GetFileInformationByHandle(h2.0, &raw mut i2) } == 0
        {
            return false;
        }
        i1.dwVolumeSerialNumber == i2.dwVolumeSerialNumber
            && i1.nFileIndexHigh == i2.nFileIndexHigh
            && i1.nFileIndexLow == i2.nFileIndexLow
            && i1.nFileSizeHigh == i2.nFileSizeHigh
            && i1.nFileSizeLow == i2.nFileSizeLow
            && i1.ftLastWriteTime.dwLowDateTime == i2.ftLastWriteTime.dwLowDateTime
            && i1.ftLastWriteTime.dwHighDateTime == i2.ftLastWriteTime.dwHighDateTime
    }
}

/// `same_file(a, b)`: whether two paths may name one file (sketch-orca, 2026-11-01). On Windows:
/// the same file once each path is read as the system opens it ([`win`]), compared without letter
/// case as NTFS compares names; then, for two existing files, whether they are one (a hard link).
/// Two names that only may be one file count as one: that refuses a run, never loses a file.
#[must_use]
pub fn same_file(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    #[cfg(windows)]
    {
        let (ka, kb) = (win::win32_file_key(a), win::win32_file_key(b));
        if win::equal_ignoring_case(&ka, &kb) {
            return true;
        }
        if win::exists(a) && win::exists(b) {
            return win::equivalent(a, b);
        }
        false
    }
    #[cfg(not(windows))]
    {
        let normal = |p: &str| {
            let full = std::path::absolute(p).unwrap_or_else(|_| std::path::PathBuf::from(p));
            std::fs::canonicalize(&full).unwrap_or(full)
        };
        if normal(a) == normal(b) {
            return true;
        }
        match (std::fs::metadata(a), std::fs::metadata(b)) {
            (Ok(x), Ok(y)) => {
                use std::os::unix::fs::MetadataExt;
                x.dev() == y.dev() && x.ino() == y.ino()
            }
            _ => false,
        }
    }
}

/// `is_file(path)`: Boost.Filesystem's `is_regular_file`, links followed; a file another program
/// holds open on Windows is of unknown type, so not a file here (as in the C++).
#[must_use]
pub fn is_file(path: &str) -> bool {
    #[cfg(windows)]
    {
        win::is_regular_file(path)
    }
    #[cfg(not(windows))]
    {
        std::fs::metadata(path).is_ok_and(|m| m.is_file())
    }
}

#[cfg(test)]
mod tests {
    use super::{is_absolute_path, same_file};

    #[test]
    fn absolute_paths_are_boosts() {
        let text = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../protocol/examples/paths/absolute.json"
        ))
        .unwrap();
        let doc = orca_json::parse(&text, orca_json::Order::Insertion).unwrap();
        let platform = if cfg!(windows) { "win32" } else { "posix" };
        let cases = doc["cases"].as_array().unwrap();
        assert!(cases.len() >= 16);
        for c in cases {
            let path = c["path"].as_str().unwrap();
            let want = c[platform].as_bool().unwrap();
            assert_eq!(is_absolute_path(path), want, "{path:?}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_names_of_one_file_are_one_file() {
        let dir = std::env::temp_dir().join(format!("orca-cli-paths-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let f = dir.join("model.stl");
        std::fs::write(&f, b"solid").unwrap();
        let p = f.to_string_lossy().into_owned();
        let d = dir.to_string_lossy().into_owned();
        assert!(same_file(&p, &p.to_uppercase()));
        assert!(same_file(&p, &format!("{d}\\sub\\..\\model.stl")));
        assert!(same_file(&p, &format!("{p}.")));
        assert!(same_file(&p, &format!("{p} ")));
        assert!(same_file(&p, &format!("\\\\?\\{p}")));
        assert!(same_file(&p, &format!("{p}::$DATA")));
        assert!(same_file(&p, &format!("{p}:progress")));
        assert!(same_file(
            &format!("{d}\\out.gcode"),
            &format!("{d}\\out.gcode.")
        ));
        let link = dir.join("link-model.stl");
        std::fs::hard_link(&f, &link).unwrap();
        assert!(same_file(&p, &link.to_string_lossy()));
        assert!(!same_file(&p, &format!("{d}\\other.stl")));
        assert!(!same_file(&p, ""));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
