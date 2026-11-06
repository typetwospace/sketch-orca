// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Files as cli/common.cpp writes them: [`write_file_atomic`] (the text into `<path>.tmp`, then
//! renamed over `<path>`), the folders it makes first, and the messages of each failure, Windows'
//! own text for a system error as Boost.System gives it.

use std::path::Path;

/// A failure, with the message the C++ `std::runtime_error` carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileError(pub String);

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// `boost::system::system_category().message(code)` on Windows: `FormatMessage`'s text without its
/// trailing line ends and full stop.
#[must_use]
pub fn system_message(code: i32) -> String {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Diagnostics::Debug::{
            FORMAT_MESSAGE_FROM_SYSTEM, FORMAT_MESSAGE_IGNORE_INSERTS, FormatMessageW,
        };
        let mut buf = [0u16; 1024];
        // SAFETY: the buffer is valid for its length, and no inserts are read.
        let n = unsafe {
            FormatMessageW(
                FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS,
                std::ptr::null(),
                code.cast_unsigned(),
                0,
                buf.as_mut_ptr(),
                u32::try_from(buf.len()).expect("a small buffer"),
                std::ptr::null(),
            )
        };
        let n = usize::try_from(n).expect("u32 fits usize");
        if n == 0 {
            return format!("Unknown error ({code})");
        }
        let mut text = String::from_utf16_lossy(&buf[..n]);
        while text.ends_with('\n') || text.ends_with('\r') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
        text
    }
    #[cfg(not(windows))]
    {
        std::io::Error::from_raw_os_error(code).to_string()
    }
}

fn os_message(e: &std::io::Error) -> String {
    e.raw_os_error()
        .map_or_else(|| e.to_string(), system_message)
}

/// `make_directories(path)`: the folder and its parents; nothing for an empty path.
///
/// # Errors
///
/// "cannot create the folder <path>: <system message>" when it is not a folder afterwards.
pub fn make_directories(path: &Path) -> Result<(), FileError> {
    if path.as_os_str().is_empty() {
        return Ok(());
    }
    match std::fs::create_dir_all(path) {
        Ok(()) => Ok(()),
        Err(_) if path.is_dir() => Ok(()),
        Err(e) => Err(FileError(format!(
            "cannot create the folder {}: {}",
            path.display(),
            os_message(&e)
        ))),
    }
}

/// `write_file_atomic(path, text)`: the bytes into `<path>.tmp`, then renamed over `path`.
///
/// # Errors
///
/// "cannot create the folder ...", "cannot create <path>.tmp", "cannot write <path>.tmp" or
/// "cannot replace <path>", as the C++ throws them.
pub fn write_file_atomic(path: &str, text: &[u8]) -> Result<(), FileError> {
    let target = Path::new(path);
    if let Some(parent) = target.parent() {
        make_directories(parent)?;
    }
    let tmp = format!("{path}.tmp");
    {
        use std::io::Write;
        let mut out =
            std::fs::File::create(&tmp).map_err(|_| FileError(format!("cannot create {tmp}")))?;
        out.write_all(text)
            .and_then(|()| out.flush())
            .map_err(|_| FileError(format!("cannot write {tmp}")))?;
    }
    if std::fs::rename(&tmp, target).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return Err(FileError(format!("cannot replace {path}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{system_message, write_file_atomic};

    #[test]
    fn a_system_message_has_no_full_stop_or_line_end() {
        let m = system_message(3);
        assert!(
            !m.is_empty() && !m.ends_with('.') && !m.ends_with('\n'),
            "{m:?}"
        );
    }

    #[test]
    fn a_file_is_written_whole_and_replaces_what_was_there() {
        let dir = std::env::temp_dir().join(format!("orca-cli-files-{}", std::process::id()));
        let path = dir.join("sub").join("out.json");
        let p = path.to_string_lossy().into_owned();
        write_file_atomic(&p, b"one").unwrap();
        write_file_atomic(&p, b"two\n").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two\n");
        assert!(!dir.join("sub").join("out.json.tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
