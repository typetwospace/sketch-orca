// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: open_zip, open_zip_reader, close_zip, close_zip_reader and class
// MZ_Archive (its constructor, get_errorstr and is_alive) of src/libslic3r/miniz_extension.cpp and
// miniz_extension.hpp, of OrcaSlicer v2.4.2, translated on 2026-11-07. Copyright the OrcaSlicer
// authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research),
// under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! libslic3r's wrapping of the zip reader: an archive opened from a path (`boost::nowide::fopen`
//! of the UTF-8 path, then `mz_zip_reader_init_cfile`) and closed again, and `MZ_Archive`'s error
//! text.

use std::fs::File;
use std::path::Path;

use crate::error::{ZipError, get_errorstr};
use crate::reader::{ZipArchive, ZipMode};

/// `open_zip_reader(zip, fname_utf8)`: `false` with `MZ_ZIP_FILE_OPEN_FAILED` when the file does
/// not open (a folder does not, as `_wfopen` refuses one), or with the reader's error, the file
/// closed again.
pub fn open_zip_reader(zip: &mut ZipArchive, fname: &Path) -> bool {
    let file = match File::open(fname) {
        Ok(f) if !f.metadata().is_ok_and(|m| m.is_dir()) => f,
        _ => {
            zip.m_last_error = ZipError::FileOpenFailed;
            return false;
        }
    };
    zip.reader_init_cfile(file, 0, 0)
}

/// `close_zip_reader(zip)`: `mz_zip_reader_end`, then the file closed.
pub fn close_zip_reader(zip: &mut ZipArchive) -> bool {
    zip.reader_end()
}

/// `MZ_Archive`: an archive zeroed at construction.
#[derive(Default)]
pub struct MzArchive {
    /// `arch`.
    pub arch: ZipArchive,
}

impl MzArchive {
    /// `MZ_Archive::get_errorstr(mz_err)`.
    #[must_use]
    pub fn errorstr_of(mz_err: ZipError) -> String {
        get_errorstr(mz_err.code())
    }

    /// `get_errorstr()`: the last error's text and `!`.
    #[must_use]
    pub fn get_errorstr(&self) -> String {
        get_errorstr(self.arch.m_last_error.code()) + "!"
    }

    /// `is_alive()`.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.arch.m_zip_mode != ZipMode::WritingHasBeenFinalized
    }
}
