// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries miniz code: the mz_zip_error enumeration of deps_src/miniz/miniz.h and
// mz_zip_get_error_string of miniz.c (miniz 2.1.0, MZ_VERSION "10.1.0") as OrcaSlicer v2.4.2
// carries it, translated on 2026-11-07. Copyright 2013-2014 RAD Game Tools and Valve Software and
// 2010-2014 Rich Geldreich and Tenacious Software LLC (the holders), under the MIT licence
// (licenses/MIT-miniz.txt).
//
// Carries OrcaSlicer code: MZ_Archive::get_errorstr of src/libslic3r/miniz_extension.cpp and
// miniz_extension.hpp, of OrcaSlicer v2.4.2, translated on 2026-11-07. Copyright the OrcaSlicer
// authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research),
// under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! The zip errors: miniz's codes, its own texts (`mz_zip_get_error_string`) and the texts
//! libslic3r reports (`MZ_Archive::get_errorstr`, which the command line does not translate:
//! `L()` returns its text as it is while no translation function is set).

/// `mz_zip_error`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum ZipError {
    /// `MZ_ZIP_NO_ERROR`.
    #[default]
    NoError = 0,
    /// `MZ_ZIP_UNDEFINED_ERROR`.
    UndefinedError,
    /// `MZ_ZIP_TOO_MANY_FILES`.
    TooManyFiles,
    /// `MZ_ZIP_FILE_TOO_LARGE`.
    FileTooLarge,
    /// `MZ_ZIP_UNSUPPORTED_METHOD`.
    UnsupportedMethod,
    /// `MZ_ZIP_UNSUPPORTED_ENCRYPTION`.
    UnsupportedEncryption,
    /// `MZ_ZIP_UNSUPPORTED_FEATURE`.
    UnsupportedFeature,
    /// `MZ_ZIP_FAILED_FINDING_CENTRAL_DIR`.
    FailedFindingCentralDir,
    /// `MZ_ZIP_NOT_AN_ARCHIVE`.
    NotAnArchive,
    /// `MZ_ZIP_INVALID_HEADER_OR_CORRUPTED`.
    InvalidHeaderOrCorrupted,
    /// `MZ_ZIP_UNSUPPORTED_MULTIDISK`.
    UnsupportedMultidisk,
    /// `MZ_ZIP_DECOMPRESSION_FAILED`.
    DecompressionFailed,
    /// `MZ_ZIP_COMPRESSION_FAILED`.
    CompressionFailed,
    /// `MZ_ZIP_UNEXPECTED_DECOMPRESSED_SIZE`.
    UnexpectedDecompressedSize,
    /// `MZ_ZIP_CRC_CHECK_FAILED`.
    CrcCheckFailed,
    /// `MZ_ZIP_UNSUPPORTED_CDIR_SIZE`.
    UnsupportedCdirSize,
    /// `MZ_ZIP_ALLOC_FAILED`.
    AllocFailed,
    /// `MZ_ZIP_FILE_OPEN_FAILED`.
    FileOpenFailed,
    /// `MZ_ZIP_FILE_CREATE_FAILED`.
    FileCreateFailed,
    /// `MZ_ZIP_FILE_WRITE_FAILED`.
    FileWriteFailed,
    /// `MZ_ZIP_FILE_READ_FAILED`.
    FileReadFailed,
    /// `MZ_ZIP_FILE_CLOSE_FAILED`.
    FileCloseFailed,
    /// `MZ_ZIP_FILE_SEEK_FAILED`.
    FileSeekFailed,
    /// `MZ_ZIP_FILE_STAT_FAILED`.
    FileStatFailed,
    /// `MZ_ZIP_INVALID_PARAMETER`.
    InvalidParameter,
    /// `MZ_ZIP_INVALID_FILENAME`.
    InvalidFilename,
    /// `MZ_ZIP_BUF_TOO_SMALL`.
    BufTooSmall,
    /// `MZ_ZIP_INTERNAL_ERROR`.
    InternalError,
    /// `MZ_ZIP_FILE_NOT_FOUND`.
    FileNotFound,
    /// `MZ_ZIP_ARCHIVE_TOO_LARGE`.
    ArchiveTooLarge,
    /// `MZ_ZIP_VALIDATION_FAILED`.
    ValidationFailed,
    /// `MZ_ZIP_WRITE_CALLBACK_FAILED`.
    WriteCallbackFailed,
    /// `MZ_ZIP_TOTAL_ERRORS`.
    TotalErrors,
}

impl ZipError {
    /// The C enumerator's value.
    #[must_use]
    pub fn code(self) -> i32 {
        self as i32
    }
}

/// `mz_zip_get_error_string(mz_err)`, of a code that may lie outside the enumeration.
#[must_use]
pub fn mz_zip_get_error_string(mz_err: i32) -> &'static str {
    match mz_err {
        0 => "no error",
        1 => "undefined error",
        2 => "too many files",
        3 => "file too large",
        4 => "unsupported method",
        5 => "unsupported encryption",
        6 => "unsupported feature",
        7 => "failed finding central directory",
        8 => "not a ZIP archive",
        9 => "invalid header or archive is corrupted",
        10 => "unsupported multidisk archive",
        11 => "decompression failed or archive is corrupted",
        12 => "compression failed",
        13 => "unexpected decompressed size",
        14 => "CRC-32 check failed",
        15 => "unsupported central directory size",
        16 => "allocation failed",
        17 => "file open failed",
        18 => "file create failed",
        19 => "file write failed",
        20 => "file read failed",
        21 => "file close failed",
        22 => "file seek failed",
        23 => "file stat failed",
        24 => "invalid parameter",
        25 => "invalid filename",
        26 => "buffer too small",
        27 => "internal error",
        28 => "file not found",
        29 => "archive is too large",
        30 => "validation failed",
        31 => "write calledback failed",
        _ => "unknown error",
    }
}

/// `MZ_Archive::get_errorstr(mz_err)`, of a code that may lie outside the enumeration, untranslated
/// as the command line has it.
#[must_use]
pub fn get_errorstr(mz_err: i32) -> String {
    match mz_err {
        0 => "no error",
        1 => "undefined error",
        2 => "too many files",
        3 => "file too large",
        4 => "unsupported method",
        5 => "unsupported encryption",
        6 => "unsupported feature",
        7 => "failed finding central directory",
        8 => "not a ZIP archive",
        9 => "invalid header or corrupted",
        10 => "unsupported multidisk",
        11 => "decompression failed",
        12 => "compression failed",
        13 => "unexpected decompressed size",
        14 => "CRC check failed",
        15 => "unsupported central directory size",
        16 => "allocation failed",
        17 => "file open failed",
        18 => "file create failed",
        19 => "file write failed",
        20 => "file read failed",
        21 => "file close failed",
        22 => "file seek failed",
        23 => "file stat failed",
        24 => "invalid parameter",
        25 => "invalid filename",
        26 => "buffer too small",
        27 => "internal error",
        28 => "file not found",
        29 => "archive too large",
        30 => "validation failed",
        31 => "write callback failed",
        _ => "unknown error",
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_codes_are_the_enumerators() {
        assert_eq!(ZipError::NoError.code(), 0);
        assert_eq!(ZipError::FileNotFound.code(), 28);
        assert_eq!(ZipError::TotalErrors.code(), 32);
        assert_eq!(mz_zip_get_error_string(32), "unknown error");
        assert_eq!(get_errorstr(9), "invalid header or corrupted");
    }
}
