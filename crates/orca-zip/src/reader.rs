// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries miniz code: the .ZIP archive reading of deps_src/miniz/miniz.c and miniz.h (miniz
// 2.1.0, MZ_VERSION "10.1.0", with Bambu Lab's mz_zip_reader_get_extra and m_is_utf8) as
// OrcaSlicer v2.4.2 carries it, translated on 2026-11-07: mz_zip_archive, mz_zip_archive_file_stat,
// mz_zip_zero_struct, mz_zip_reader_init_internal, mz_zip_reader_filename_less,
// mz_zip_reader_sort_central_dir_offsets_by_filename, mz_zip_reader_locate_header_sig,
// mz_zip_reader_read_central_dir, mz_zip_reader_end_internal, mz_zip_reader_end,
// mz_zip_reader_init, mz_zip_mem_read_func, mz_zip_reader_init_mem, mz_zip_file_read_func,
// mz_zip_reader_init_cfile, mz_zip_get_cdh, mz_zip_reader_is_file_encrypted,
// mz_zip_reader_is_file_supported, mz_zip_reader_is_file_a_directory, mz_zip_file_stat_internal,
// mz_zip_string_equal, mz_zip_filename_compare, mz_zip_locate_file_binary_search,
// mz_zip_reader_locate_file, mz_zip_reader_locate_file_v2, mz_zip_reader_extract_to_mem_no_alloc,
// mz_zip_reader_extract_file_to_mem_no_alloc, mz_zip_reader_extract_to_mem,
// mz_zip_reader_extract_file_to_mem, mz_zip_reader_extract_to_callback,
// mz_zip_reader_extract_file_to_callback, mz_zip_file_write_callback,
// mz_zip_reader_extract_to_file, mz_zip_reader_get_num_files, mz_zip_get_cfile,
// mz_zip_reader_get_extra and mz_zip_reader_file_stat. Copyright 2013-2014 RAD Game Tools and
// Valve Software and 2010-2014 Rich Geldreich and Tenacious Software LLC (the holders), under the
// MIT licence (licenses/MIT-miniz.txt).

//! miniz's zip reader on the 3MF reader's path: an archive opened from a file, from memory or
//! through a read function; its central directory read and checked (with ZIP64) and its names
//! sorted for a search without case; an entry's statistics, extra field, and its data extracted
//! to a buffer, through a callback or to a file, with the CRC-32 checked.
//!
//! Each function keeps the C's checks in the C's order and leaves the same error in
//! `m_last_error`, including the C's slips: the central directory's "stored" check reads the
//! method and the file time as one 32-bit word, a ZIP64 extra field that runs past the directory
//! is read from the first record's place, the end of central directory search walks to a
//! negative offset (and fails) in an archive of 4097 to 8188 bytes whose record is not in the last
//! 4096, and the end of a callback extraction reports a CRC mismatch as a decompression failure.
//! `m_time` is left out: miniz makes it with `mktime` in the local time zone, and nothing on the
//! command line's path reads it (an extracted file keeps the time it was written at;
//! `parity/HAZARDS.md`, orca-zip). The read buffer of an extraction is `MZ_ZIP_MAX_IO_BUF_SIZE`,
//! and a callback extraction inflates into a 32 KiB ring, so the chunks a callback sees are the
//! C's.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

use crate::crc::{MZ_CRC32_INIT, mz_crc32};
use crate::error::ZipError;
use crate::tinfl::{
    Decompressor, Status, TINFL_FLAG_HAS_MORE_INPUT, TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
    TINFL_LZ_DICT_SIZE,
};

/// `MZ_ZIP_FLAG_CASE_SENSITIVE`.
pub const MZ_ZIP_FLAG_CASE_SENSITIVE: u32 = 0x0100;
/// `MZ_ZIP_FLAG_IGNORE_PATH`.
pub const MZ_ZIP_FLAG_IGNORE_PATH: u32 = 0x0200;
/// `MZ_ZIP_FLAG_COMPRESSED_DATA`.
pub const MZ_ZIP_FLAG_COMPRESSED_DATA: u32 = 0x0400;
/// `MZ_ZIP_FLAG_DO_NOT_SORT_CENTRAL_DIRECTORY`.
pub const MZ_ZIP_FLAG_DO_NOT_SORT_CENTRAL_DIRECTORY: u32 = 0x0800;

/// `MZ_ZIP_MAX_IO_BUF_SIZE`.
pub const MZ_ZIP_MAX_IO_BUF_SIZE: u64 = 64 * 1024;
/// `MZ_ZIP_MAX_ARCHIVE_FILENAME_SIZE`.
pub const MZ_ZIP_MAX_ARCHIVE_FILENAME_SIZE: usize = 512;
/// `MZ_ZIP_MAX_ARCHIVE_FILE_COMMENT_SIZE`.
pub const MZ_ZIP_MAX_ARCHIVE_FILE_COMMENT_SIZE: usize = 512;

const MZ_DEFLATED: u16 = 8;
const MZ_UINT16_MAX: u64 = 0xFFFF;
const MZ_UINT32_MAX: u64 = 0xFFFF_FFFF;

const END_OF_CENTRAL_DIR_HEADER_SIG: u32 = 0x0605_4b50;
const CENTRAL_DIR_HEADER_SIG: u32 = 0x0201_4b50;
const LOCAL_DIR_HEADER_SIG: u32 = 0x0403_4b50;
const LOCAL_DIR_HEADER_SIZE: usize = 30;
const CENTRAL_DIR_HEADER_SIZE: usize = 46;
const END_OF_CENTRAL_DIR_HEADER_SIZE: usize = 22;
const ZIP64_END_OF_CENTRAL_DIR_HEADER_SIG: u32 = 0x0606_4b50;
const ZIP64_END_OF_CENTRAL_DIR_LOCATOR_SIG: u32 = 0x0706_4b50;
const ZIP64_END_OF_CENTRAL_DIR_HEADER_SIZE: usize = 56;
const ZIP64_END_OF_CENTRAL_DIR_LOCATOR_SIZE: usize = 20;
const ZIP64_EXTENDED_INFORMATION_FIELD_HEADER_ID: u32 = 0x0001;

const CDH_VERSION_MADE_BY_OFS: usize = 4;
const CDH_VERSION_NEEDED_OFS: usize = 6;
const CDH_BIT_FLAG_OFS: usize = 8;
const CDH_METHOD_OFS: usize = 10;
const CDH_CRC32_OFS: usize = 16;
const CDH_COMPRESSED_SIZE_OFS: usize = 20;
const CDH_DECOMPRESSED_SIZE_OFS: usize = 24;
const CDH_FILENAME_LEN_OFS: usize = 28;
const CDH_EXTRA_LEN_OFS: usize = 30;
const CDH_COMMENT_LEN_OFS: usize = 32;
const CDH_DISK_START_OFS: usize = 34;
const CDH_INTERNAL_ATTR_OFS: usize = 36;
const CDH_EXTERNAL_ATTR_OFS: usize = 38;
const CDH_LOCAL_HEADER_OFS: usize = 42;

const LDH_FILENAME_LEN_OFS: usize = 26;
const LDH_EXTRA_LEN_OFS: usize = 28;

const ECDH_NUM_THIS_DISK_OFS: usize = 4;
const ECDH_NUM_DISK_CDIR_OFS: usize = 6;
const ECDH_CDIR_NUM_ENTRIES_ON_DISK_OFS: usize = 8;
const ECDH_CDIR_TOTAL_ENTRIES_OFS: usize = 10;
const ECDH_CDIR_SIZE_OFS: usize = 12;
const ECDH_CDIR_OFS_OFS: usize = 16;

const ZIP64_ECDL_REL_OFS_TO_ZIP64_ECDR_OFS: usize = 8;
const ZIP64_ECDL_TOTAL_NUMBER_OF_DISKS_OFS: usize = 16;
const ZIP64_ECDH_SIZE_OF_RECORD_OFS: usize = 4;
const ZIP64_ECDH_NUM_THIS_DISK_OFS: usize = 16;
const ZIP64_ECDH_NUM_DISK_CDIR_OFS: usize = 20;
const ZIP64_ECDH_CDIR_NUM_ENTRIES_ON_DISK_OFS: usize = 24;
const ZIP64_ECDH_CDIR_TOTAL_ENTRIES_OFS: usize = 32;
const ZIP64_ECDH_CDIR_SIZE_OFS: usize = 40;
const ZIP64_ECDH_CDIR_OFS_OFS: usize = 48;

const DOS_DIR_ATTRIBUTE_BITFLAG: u32 = 0x10;
const FLAG_IS_ENCRYPTED: u16 = 1;
const FLAG_COMPRESSED_PATCH_FLAG: u16 = 32;
const FLAG_USES_STRONG_ENCRYPTION: u16 = 64;
const FLAG_LOCAL_DIR_IS_MASKED: u32 = 8192;
const FLAG_UTF8: u16 = 1 << 11;

fn le16(b: &[u8], at: usize) -> u32 {
    u32::from(b[at]) | (u32::from(b[at + 1]) << 8)
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn le64(b: &[u8], at: usize) -> u64 {
    u64::from(le32(b, at)) | (u64::from(le32(b, at + 4)) << 32)
}

/// `MZ_TOLOWER`.
fn tolower(c: u8) -> u8 {
    if c.is_ascii_uppercase() {
        c - b'A' + b'a'
    } else {
        c
    }
}

/// A C string's bytes: up to the first NUL.
fn cstr(s: &[u8]) -> &[u8] {
    s.iter().position(|&c| c == 0).map_or(s, |n| &s[..n])
}

/// `MZ_MAX` of two integers.
fn umax(a: u64, b: u64) -> u64 {
    if a < b { b } else { a }
}

/// `mz_zip_mode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ZipMode {
    /// `MZ_ZIP_MODE_INVALID`.
    #[default]
    Invalid,
    /// `MZ_ZIP_MODE_READING`.
    Reading,
    /// `MZ_ZIP_MODE_WRITING`.
    Writing,
    /// `MZ_ZIP_MODE_WRITING_HAS_BEEN_FINALIZED`.
    WritingHasBeenFinalized,
}

/// `mz_zip_type`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ZipType {
    /// `MZ_ZIP_TYPE_INVALID`.
    #[default]
    Invalid,
    /// `MZ_ZIP_TYPE_USER`.
    User,
    /// `MZ_ZIP_TYPE_MEMORY`.
    Memory,
    /// `MZ_ZIP_TYPE_HEAP`.
    Heap,
    /// `MZ_ZIP_TYPE_FILE`.
    File,
    /// `MZ_ZIP_TYPE_CFILE`.
    CFile,
}

/// A read function (`mz_file_read_func`): reads up to `buf.len()` bytes at an offset and returns
/// how many it read.
pub type ReadFunc = Box<dyn FnMut(u64, &mut [u8]) -> usize>;

/// A write function (`mz_file_write_func`): takes a chunk's offset and bytes and returns how many
/// it took, or an error that ends the extraction (the C++ callback's exception).
pub type WriteFunc<'a, E> = dyn FnMut(u64, &[u8]) -> Result<usize, E> + 'a;

/// Where an archive's bytes come from: `m_pRead` with what it reads (`m_pState->m_pMem` or
/// `m_pFile`).
enum Source {
    /// No read function.
    None,
    /// A caller's read function (`mz_zip_reader_init`).
    User(ReadFunc),
    /// The archive in memory (`mz_zip_reader_init_mem`).
    Mem(Vec<u8>),
    /// An open file (`mz_zip_reader_init_cfile`) and where the archive starts in it.
    CFile { file: File, start_ofs: u64 },
}

/// `mz_zip_internal_state`, as the reader uses it.
#[derive(Default)]
struct State {
    central_dir: Vec<u8>,
    central_dir_offsets: Vec<u32>,
    sorted_central_dir_offsets: Vec<u32>,
    init_flags: u32,
    zip64: bool,
    zip64_has_extended_info_fields: bool,
}

/// `mz_zip_archive_file_stat` (but `m_time`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileStat {
    /// `m_file_index`.
    pub m_file_index: u32,
    /// `m_central_dir_ofs`.
    pub m_central_dir_ofs: u64,
    /// `m_version_made_by`.
    pub m_version_made_by: u16,
    /// `m_version_needed`.
    pub m_version_needed: u16,
    /// `m_bit_flag`.
    pub m_bit_flag: u16,
    /// `m_method`.
    pub m_method: u16,
    /// `m_crc32`.
    pub m_crc32: u32,
    /// `m_comp_size`.
    pub m_comp_size: u64,
    /// `m_uncomp_size`.
    pub m_uncomp_size: u64,
    /// `m_internal_attr`.
    pub m_internal_attr: u16,
    /// `m_external_attr`.
    pub m_external_attr: u32,
    /// `m_local_header_ofs`.
    pub m_local_header_ofs: u64,
    /// `m_comment_size`.
    pub m_comment_size: u32,
    /// `m_is_directory`.
    pub m_is_directory: bool,
    /// `m_is_encrypted`.
    pub m_is_encrypted: bool,
    /// `m_is_supported`.
    pub m_is_supported: bool,
    /// `m_is_utf8`: the bit flag's UTF-8 bit as it stands (0 or 2048).
    pub m_is_utf8: i32,
    /// `m_filename` as a C string: the name's first 511 bytes, up to a NUL.
    pub m_filename: Vec<u8>,
    /// `m_comment` as a C string: the comment's first 511 bytes, up to a NUL.
    pub m_comment: Vec<u8>,
}

/// `mz_zip_archive`, as the reader uses it; [`ZipArchive::default`] is `mz_zip_zero_struct`.
pub struct ZipArchive {
    /// `m_archive_size`.
    pub m_archive_size: u64,
    /// `m_central_directory_file_ofs`.
    pub m_central_directory_file_ofs: u64,
    /// `m_total_files`.
    pub m_total_files: u32,
    /// `m_zip_mode`.
    pub m_zip_mode: ZipMode,
    /// `m_zip_type`.
    pub m_zip_type: ZipType,
    /// `m_last_error`.
    pub m_last_error: ZipError,
    source: Source,
    state: Option<State>,
}

impl Default for ZipArchive {
    fn default() -> Self {
        ZipArchive {
            m_archive_size: 0,
            m_central_directory_file_ofs: 0,
            m_total_files: 0,
            m_zip_mode: ZipMode::Invalid,
            m_zip_type: ZipType::Invalid,
            m_last_error: ZipError::NoError,
            source: Source::None,
            state: None,
        }
    }
}

/// The input of an extraction: the archive's memory, or a read buffer.
enum Input {
    Mem { ofs: usize },
    Buf(Vec<u8>),
}

impl ZipArchive {
    /// `mz_zip_set_error`: sets the error and returns `false`.
    fn set_error(&mut self, err: ZipError) -> bool {
        self.m_last_error = err;
        false
    }

    /// `m_pRead != NULL`.
    fn has_read(&self) -> bool {
        !matches!(self.source, Source::None)
    }

    /// `m_pState->m_pMem != NULL`.
    fn is_mem(&self) -> bool {
        matches!(self.source, Source::Mem(_))
    }

    /// `m_pRead(m_pIO_opaque, file_ofs, buf, buf.len())`.
    fn read(&mut self, file_ofs: u64, buf: &mut [u8]) -> usize {
        let archive_size = self.m_archive_size;
        match &mut self.source {
            Source::None => 0,
            Source::User(f) => f(file_ofs, buf),
            Source::Mem(mem) => {
                // mz_zip_mem_read_func.
                let s = if file_ofs >= archive_size {
                    0
                } else {
                    Ord::min(archive_size - file_ofs, buf.len() as u64) as usize
                };
                if s == 0 {
                    // memcpy of no bytes, from wherever the offset points.
                    return 0;
                }
                let at = file_ofs as usize;
                buf[..s].copy_from_slice(&mem[at..at + s]);
                s
            }
            Source::CFile { file, start_ofs } => {
                // mz_zip_file_read_func: a seek to the place, then fread.
                let ofs = file_ofs.wrapping_add(*start_ofs);
                if (ofs as i64) < 0 || file.seek(SeekFrom::Start(ofs)).is_err() {
                    return 0;
                }
                let mut got = 0;
                while got < buf.len() {
                    match file.read(&mut buf[got..]) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => got += n,
                    }
                }
                got
            }
        }
    }

    /// `mz_zip_reader_init_internal`.
    fn init_internal(&mut self, flags: u32) -> bool {
        if self.state.is_some() || self.m_zip_mode != ZipMode::Invalid {
            return self.set_error(ZipError::InvalidParameter);
        }
        self.m_archive_size = 0;
        self.m_central_directory_file_ofs = 0;
        self.m_total_files = 0;
        self.m_last_error = ZipError::NoError;
        self.state = Some(State {
            init_flags: flags,
            ..State::default()
        });
        self.m_zip_mode = ZipMode::Reading;
        true
    }

    /// `mz_zip_reader_locate_header_sig`: the offset of the last `record_sig` with room for
    /// `record_size` bytes after it, searched backwards 4096 bytes at a time.
    fn locate_header_sig(&mut self, record_sig: u32, record_size: u64) -> Option<i64> {
        const BUF: usize = 4096;
        let mut buf = [0u8; BUF];
        if self.m_archive_size < record_size {
            return None;
        }
        let size = self.m_archive_size;
        let mut cur_file_ofs: i64 = {
            let d = (size as i64).wrapping_sub(BUF as i64);
            if d < 0 { 0 } else { d }
        };
        loop {
            let n = Ord::min(BUF as u64, size.wrapping_sub(cur_file_ofs as u64)) as i32;
            if self.read(cur_file_ofs as u64, &mut buf[..n as usize]) != n as usize {
                return None;
            }
            let mut i = n - 4;
            while i >= 0 {
                let s = le32(&buf, i as usize);
                if s == record_sig
                    && size.wrapping_sub(cur_file_ofs.wrapping_add(i64::from(i)) as u64)
                        >= record_size
                {
                    break;
                }
                i -= 1;
            }
            if i >= 0 {
                cur_file_ofs += i64::from(i);
                break;
            }
            if cur_file_ofs == 0
                || size.wrapping_sub(cur_file_ofs as u64) >= MZ_UINT16_MAX + record_size
            {
                return None;
            }
            // MZ_MAX(cur_file_ofs - (sizeof(buf_u32) - 3), 0): an unsigned difference, so an
            // offset under 4093 wraps instead of stopping at 0.
            cur_file_ofs = umax((cur_file_ofs as u64).wrapping_sub(BUF as u64 - 3), 0) as i64;
        }
        Some(cur_file_ofs)
    }

    /// `mz_zip_reader_read_central_dir`.
    #[allow(clippy::too_many_lines)]
    fn read_central_dir(&mut self, flags: u32) -> bool {
        let sort_central_dir = (flags & MZ_ZIP_FLAG_DO_NOT_SORT_CENTRAL_DIRECTORY) == 0;
        if self.m_archive_size < END_OF_CENTRAL_DIR_HEADER_SIZE as u64 {
            return self.set_error(ZipError::NotAnArchive);
        }
        let Some(cur_file_ofs) = self.locate_header_sig(
            END_OF_CENTRAL_DIR_HEADER_SIG,
            END_OF_CENTRAL_DIR_HEADER_SIZE as u64,
        ) else {
            return self.set_error(ZipError::FailedFindingCentralDir);
        };
        let mut buf = [0u8; END_OF_CENTRAL_DIR_HEADER_SIZE];
        if self.read(cur_file_ofs as u64, &mut buf) != END_OF_CENTRAL_DIR_HEADER_SIZE {
            return self.set_error(ZipError::FileReadFailed);
        }
        if le32(&buf, 0) != END_OF_CENTRAL_DIR_HEADER_SIG {
            return self.set_error(ZipError::NotAnArchive);
        }
        let mut locator = [0u8; ZIP64_END_OF_CENTRAL_DIR_LOCATOR_SIZE];
        let mut z64 = [0u8; ZIP64_END_OF_CENTRAL_DIR_HEADER_SIZE];
        let mut zip64 = false;
        if cur_file_ofs
            >= (ZIP64_END_OF_CENTRAL_DIR_LOCATOR_SIZE + ZIP64_END_OF_CENTRAL_DIR_HEADER_SIZE) as i64
            && self.read(
                (cur_file_ofs - ZIP64_END_OF_CENTRAL_DIR_LOCATOR_SIZE as i64) as u64,
                &mut locator,
            ) == ZIP64_END_OF_CENTRAL_DIR_LOCATOR_SIZE
            && le32(&locator, 0) == ZIP64_END_OF_CENTRAL_DIR_LOCATOR_SIG
        {
            let z64_ofs = le64(&locator, ZIP64_ECDL_REL_OFS_TO_ZIP64_ECDR_OFS);
            if z64_ofs > self.m_archive_size - ZIP64_END_OF_CENTRAL_DIR_HEADER_SIZE as u64 {
                return self.set_error(ZipError::NotAnArchive);
            }
            if self.read(z64_ofs, &mut z64) == ZIP64_END_OF_CENTRAL_DIR_HEADER_SIZE
                && le32(&z64, 0) == ZIP64_END_OF_CENTRAL_DIR_HEADER_SIG
            {
                zip64 = true;
            }
        }
        if let Some(st) = self.state.as_mut() {
            st.zip64 = zip64;
        }

        self.m_total_files = le16(&buf, ECDH_CDIR_TOTAL_ENTRIES_OFS);
        let mut cdir_entries_on_this_disk = le16(&buf, ECDH_CDIR_NUM_ENTRIES_ON_DISK_OFS);
        let mut num_this_disk = le16(&buf, ECDH_NUM_THIS_DISK_OFS);
        let mut cdir_disk_index = le16(&buf, ECDH_NUM_DISK_CDIR_OFS);
        let mut cdir_size = le32(&buf, ECDH_CDIR_SIZE_OFS);
        let mut cdir_ofs = u64::from(le32(&buf, ECDH_CDIR_OFS_OFS));

        if zip64 {
            let total_disks = le32(&locator, ZIP64_ECDL_TOTAL_NUMBER_OF_DISKS_OFS);
            let total_entries = le64(&z64, ZIP64_ECDH_CDIR_TOTAL_ENTRIES_OFS);
            let entries_on_this_disk = le64(&z64, ZIP64_ECDH_CDIR_NUM_ENTRIES_ON_DISK_OFS);
            let size_of_record = le64(&z64, ZIP64_ECDH_SIZE_OF_RECORD_OFS);
            let size_of_cdir = le64(&z64, ZIP64_ECDH_CDIR_SIZE_OFS);
            if size_of_record < (ZIP64_END_OF_CENTRAL_DIR_HEADER_SIZE - 12) as u64 {
                return self.set_error(ZipError::InvalidHeaderOrCorrupted);
            }
            if total_disks != 1 {
                return self.set_error(ZipError::UnsupportedMultidisk);
            }
            if total_entries > MZ_UINT32_MAX {
                return self.set_error(ZipError::TooManyFiles);
            }
            self.m_total_files = total_entries as u32;
            if entries_on_this_disk > MZ_UINT32_MAX {
                return self.set_error(ZipError::TooManyFiles);
            }
            cdir_entries_on_this_disk = entries_on_this_disk as u32;
            if size_of_cdir > MZ_UINT32_MAX {
                return self.set_error(ZipError::UnsupportedCdirSize);
            }
            cdir_size = size_of_cdir as u32;
            num_this_disk = le32(&z64, ZIP64_ECDH_NUM_THIS_DISK_OFS);
            cdir_disk_index = le32(&z64, ZIP64_ECDH_NUM_DISK_CDIR_OFS);
            cdir_ofs = le64(&z64, ZIP64_ECDH_CDIR_OFS_OFS);
        }

        if self.m_total_files != cdir_entries_on_this_disk {
            return self.set_error(ZipError::UnsupportedMultidisk);
        }
        if (num_this_disk | cdir_disk_index) != 0 && (num_this_disk != 1 || cdir_disk_index != 1) {
            return self.set_error(ZipError::UnsupportedMultidisk);
        }
        if cdir_size
            < self
                .m_total_files
                .wrapping_mul(CENTRAL_DIR_HEADER_SIZE as u32)
        {
            return self.set_error(ZipError::InvalidHeaderOrCorrupted);
        }
        if cdir_ofs.wrapping_add(u64::from(cdir_size)) > self.m_archive_size {
            return self.set_error(ZipError::InvalidHeaderOrCorrupted);
        }
        self.m_central_directory_file_ofs = cdir_ofs;

        let total = self.m_total_files;
        if total != 0 {
            let mut central_dir = vec![0u8; cdir_size as usize];
            // The C sizes both index arrays to the entry count up front; they are filled one entry
            // at a time here, which leaves the same arrays whenever the directory reads whole.
            let cap = Ord::min(total as usize, cdir_size as usize / CENTRAL_DIR_HEADER_SIZE);
            let mut offsets: Vec<u32> = Vec::with_capacity(cap);
            let mut sorted: Vec<u32> = Vec::with_capacity(if sort_central_dir { cap } else { 0 });
            if self.read(cdir_ofs, &mut central_dir) != cdir_size as usize {
                return self.set_error(ZipError::FileReadFailed);
            }
            let mut p = 0usize;
            let mut n = cdir_size as usize;
            let mut has_ext = self
                .state
                .as_ref()
                .is_some_and(|s| s.zip64_has_extended_info_fields);
            let mut zip64_now = zip64;
            for i in 0..total {
                if n < CENTRAL_DIR_HEADER_SIZE || le32(&central_dir, p) != CENTRAL_DIR_HEADER_SIG {
                    return self.set_error(ZipError::InvalidHeaderOrCorrupted);
                }
                offsets.push(p as u32);
                if sort_central_dir {
                    sorted.push(i);
                }
                let comp_size = u64::from(le32(&central_dir, p + CDH_COMPRESSED_SIZE_OFS));
                let decomp_size = u64::from(le32(&central_dir, p + CDH_DECOMPRESSED_SIZE_OFS));
                let local_header_ofs = u64::from(le32(&central_dir, p + CDH_LOCAL_HEADER_OFS));
                let filename_size = le16(&central_dir, p + CDH_FILENAME_LEN_OFS) as usize;
                let ext_data_size = le16(&central_dir, p + CDH_EXTRA_LEN_OFS) as usize;

                if !has_ext
                    && ext_data_size != 0
                    && umax(umax(comp_size, decomp_size), local_header_ofs) == MZ_UINT32_MAX
                {
                    let mut extra_size_remaining = ext_data_size;
                    let owned;
                    let extra: &[u8] =
                        if CENTRAL_DIR_HEADER_SIZE + filename_size + ext_data_size > n {
                            // Read from the archive at the first record's place, as the C reads it.
                            let mut b = vec![0u8; ext_data_size];
                            let at = cdir_ofs
                                .wrapping_add((CENTRAL_DIR_HEADER_SIZE + filename_size) as u64);
                            if self.read(at, &mut b) != ext_data_size {
                                return self.set_error(ZipError::FileReadFailed);
                            }
                            owned = b;
                            &owned
                        } else {
                            let at = p + CENTRAL_DIR_HEADER_SIZE + filename_size;
                            &central_dir[at..at + ext_data_size]
                        };
                    let mut at = 0usize;
                    loop {
                        if extra_size_remaining < 4 {
                            return self.set_error(ZipError::InvalidHeaderOrCorrupted);
                        }
                        let field_id = le16(extra, at);
                        let field_data_size = le16(extra, at + 2) as usize;
                        if field_data_size + 4 > extra_size_remaining {
                            return self.set_error(ZipError::InvalidHeaderOrCorrupted);
                        }
                        if field_id == ZIP64_EXTENDED_INFORMATION_FIELD_HEADER_ID {
                            zip64_now = true;
                            has_ext = true;
                            break;
                        }
                        at += 4 + field_data_size;
                        extra_size_remaining -= 4 + field_data_size;
                        if extra_size_remaining == 0 {
                            break;
                        }
                    }
                    if let Some(st) = self.state.as_mut() {
                        st.zip64 = zip64_now;
                        st.zip64_has_extended_info_fields = has_ext;
                    }
                }

                // The C reads the method and the file time as one 32-bit word here.
                if comp_size != MZ_UINT32_MAX
                    && decomp_size != MZ_UINT32_MAX
                    && ((le32(&central_dir, p + CDH_METHOD_OFS) == 0 && decomp_size != comp_size)
                        || (decomp_size != 0 && comp_size == 0))
                {
                    return self.set_error(ZipError::InvalidHeaderOrCorrupted);
                }
                let disk_index = le16(&central_dir, p + CDH_DISK_START_OFS);
                if u64::from(disk_index) == MZ_UINT16_MAX
                    || (disk_index != num_this_disk && disk_index != 1)
                {
                    return self.set_error(ZipError::UnsupportedMultidisk);
                }
                if comp_size != MZ_UINT32_MAX
                    && u64::from(le32(&central_dir, p + CDH_LOCAL_HEADER_OFS))
                        + LOCAL_DIR_HEADER_SIZE as u64
                        + comp_size
                        > self.m_archive_size
                {
                    return self.set_error(ZipError::InvalidHeaderOrCorrupted);
                }
                let bit_flags = le16(&central_dir, p + CDH_BIT_FLAG_OFS);
                if bit_flags & FLAG_LOCAL_DIR_IS_MASKED != 0 {
                    return self.set_error(ZipError::UnsupportedEncryption);
                }
                let total_header_size = CENTRAL_DIR_HEADER_SIZE
                    + le16(&central_dir, p + CDH_FILENAME_LEN_OFS) as usize
                    + le16(&central_dir, p + CDH_EXTRA_LEN_OFS) as usize
                    + le16(&central_dir, p + CDH_COMMENT_LEN_OFS) as usize;
                if total_header_size > n {
                    return self.set_error(ZipError::InvalidHeaderOrCorrupted);
                }
                n -= total_header_size;
                p += total_header_size;
            }
            if let Some(st) = self.state.as_mut() {
                st.central_dir = central_dir;
                st.central_dir_offsets = offsets;
                st.sorted_central_dir_offsets = sorted;
            }
        }
        if sort_central_dir {
            self.sort_central_dir_offsets_by_filename();
        }
        true
    }

    /// `mz_zip_reader_sort_central_dir_offsets_by_filename`: miniz's heap sort of the names
    /// without case.
    fn sort_central_dir_offsets_by_filename(&mut self) {
        let size = self.m_total_files;
        let Some(st) = self.state.as_mut() else {
            return;
        };
        if size <= 1 {
            return;
        }
        let cd = &st.central_dir;
        let offs = &st.central_dir_offsets;
        let idx = &mut st.sorted_central_dir_offsets;
        let less = |l: u32, r: u32| filename_less(cd, offs, l, r);
        let size64 = u64::from(size);
        let mut start = (size - 2) >> 1;
        loop {
            let mut root = u64::from(start);
            loop {
                let mut child = (root << 1) + 1;
                if child >= size64 {
                    break;
                }
                if child + 1 < size64 && less(idx[child as usize], idx[child as usize + 1]) {
                    child += 1;
                }
                if !less(idx[root as usize], idx[child as usize]) {
                    break;
                }
                idx.swap(root as usize, child as usize);
                root = child;
            }
            if start == 0 {
                break;
            }
            start -= 1;
        }
        let mut end = size - 1;
        while end > 0 {
            let mut root = 0u64;
            idx.swap(end as usize, 0);
            let end64 = u64::from(end);
            loop {
                let mut child = (root << 1) + 1;
                if child >= end64 {
                    break;
                }
                if child + 1 < end64 && less(idx[child as usize], idx[child as usize + 1]) {
                    child += 1;
                }
                if !less(idx[root as usize], idx[child as usize]) {
                    break;
                }
                idx.swap(root as usize, child as usize);
                root = child;
            }
            end -= 1;
        }
    }

    /// `mz_zip_reader_end_internal`.
    fn end_internal(&mut self, set_last_error: bool) -> bool {
        if self.state.is_none() || self.m_zip_mode != ZipMode::Reading {
            if set_last_error {
                self.m_last_error = ZipError::InvalidParameter;
            }
            return false;
        }
        self.state = None;
        // m_pFile set to NULL; the FILE of a C file is closed by close_zip_reader, which runs this
        // first, and the memory belongs to the caller: both go with the state here.
        if matches!(self.source, Source::CFile { .. } | Source::Mem(_)) {
            self.source = Source::None;
        }
        self.m_zip_mode = ZipMode::Invalid;
        true
    }

    /// `mz_zip_reader_end`.
    pub fn reader_end(&mut self) -> bool {
        self.end_internal(true)
    }

    /// `archive.m_pRead = read`: the read function [`ZipArchive::reader_init`] reads through.
    pub fn set_read_func(&mut self, read: ReadFunc) {
        self.source = Source::User(read);
    }

    /// `mz_zip_reader_init(pZip, size, flags)`, through the read function set first.
    pub fn reader_init(&mut self, size: u64, flags: u32) -> bool {
        if !matches!(self.source, Source::User(_)) {
            return self.set_error(ZipError::InvalidParameter);
        }
        if !self.init_internal(flags) {
            return false;
        }
        self.m_zip_type = ZipType::User;
        self.m_archive_size = size;
        if !self.read_central_dir(flags) {
            self.end_internal(false);
            return false;
        }
        true
    }

    /// `mz_zip_reader_init_mem(pZip, pMem, size, flags)`: the archive owns its bytes here.
    pub fn reader_init_mem(&mut self, mem: Vec<u8>, flags: u32) -> bool {
        if mem.len() < END_OF_CENTRAL_DIR_HEADER_SIZE {
            return self.set_error(ZipError::NotAnArchive);
        }
        if !self.init_internal(flags) {
            return false;
        }
        self.m_zip_type = ZipType::Memory;
        self.m_archive_size = mem.len() as u64;
        self.source = Source::Mem(mem);
        if !self.read_central_dir(flags) {
            self.end_internal(false);
            return false;
        }
        true
    }

    /// `mz_zip_reader_init_cfile(pZip, pFile, archive_size, flags)`, with the file at its start;
    /// on a failure the file is dropped (closed), as `open_zip_reader` closes it.
    pub fn reader_init_cfile(&mut self, mut file: File, archive_size: u64, flags: u32) -> bool {
        let Ok(cur_file_ofs) = file.stream_position() else {
            return self.set_error(ZipError::FileSeekFailed);
        };
        let mut archive_size = archive_size;
        if archive_size == 0 {
            let Ok(end) = file.seek(SeekFrom::End(0)) else {
                return self.set_error(ZipError::FileSeekFailed);
            };
            archive_size = end.wrapping_sub(cur_file_ofs);
            if archive_size < END_OF_CENTRAL_DIR_HEADER_SIZE as u64 {
                return self.set_error(ZipError::NotAnArchive);
            }
        }
        if !self.init_internal(flags) {
            return false;
        }
        self.m_zip_type = ZipType::CFile;
        self.source = Source::CFile {
            file,
            start_ofs: cur_file_ofs,
        };
        self.m_archive_size = archive_size;
        if !self.read_central_dir(flags) {
            self.end_internal(false);
            self.source = Source::None;
            return false;
        }
        true
    }

    /// `mz_zip_get_cfile(pZip) != NULL`.
    #[must_use]
    pub fn has_cfile(&self) -> bool {
        self.state.is_some() && matches!(self.source, Source::CFile { .. })
    }

    /// `mz_zip_is_zip64`: the archive has ZIP64 records or a ZIP64 extra field.
    #[must_use]
    pub fn is_zip64(&self) -> bool {
        self.state.as_ref().is_some_and(|s| s.zip64)
    }

    /// `mz_zip_reader_get_num_files`.
    #[must_use]
    pub fn reader_get_num_files(&self) -> u32 {
        self.m_total_files
    }

    /// `mz_zip_get_cdh`: where entry `file_index`'s central directory record starts.
    fn cdh(&self, file_index: u32) -> Option<usize> {
        let st = self.state.as_ref()?;
        if file_index >= self.m_total_files {
            return None;
        }
        Some(st.central_dir_offsets[file_index as usize] as usize)
    }

    fn cd(&self) -> &[u8] {
        self.state
            .as_ref()
            .map_or(&[], |s| s.central_dir.as_slice())
    }

    /// `mz_zip_reader_is_file_encrypted`.
    pub fn reader_is_file_encrypted(&mut self, file_index: u32) -> bool {
        let Some(p) = self.cdh(file_index) else {
            return self.set_error(ZipError::InvalidParameter);
        };
        let bit_flag = le16(self.cd(), p + CDH_BIT_FLAG_OFS) as u16;
        bit_flag & (FLAG_IS_ENCRYPTED | FLAG_USES_STRONG_ENCRYPTION) != 0
    }

    /// `mz_zip_reader_is_file_supported`.
    pub fn reader_is_file_supported(&mut self, file_index: u32) -> bool {
        let Some(p) = self.cdh(file_index) else {
            return self.set_error(ZipError::InvalidParameter);
        };
        let cd = self.cd();
        let method = le16(cd, p + CDH_METHOD_OFS) as u16;
        let bit_flag = le16(cd, p + CDH_BIT_FLAG_OFS) as u16;
        if method != 0 && method != MZ_DEFLATED {
            return self.set_error(ZipError::UnsupportedMethod);
        }
        if bit_flag & (FLAG_IS_ENCRYPTED | FLAG_USES_STRONG_ENCRYPTION) != 0 {
            return self.set_error(ZipError::UnsupportedEncryption);
        }
        if bit_flag & FLAG_COMPRESSED_PATCH_FLAG != 0 {
            return self.set_error(ZipError::UnsupportedFeature);
        }
        true
    }

    /// `mz_zip_reader_is_file_a_directory`: the name ends in `/`, or the DOS directory attribute.
    pub fn reader_is_file_a_directory(&mut self, file_index: u32) -> bool {
        let Some(p) = self.cdh(file_index) else {
            return self.set_error(ZipError::InvalidParameter);
        };
        let cd = self.cd();
        let filename_len = le16(cd, p + CDH_FILENAME_LEN_OFS) as usize;
        if filename_len != 0 && cd[p + CENTRAL_DIR_HEADER_SIZE + filename_len - 1] == b'/' {
            return true;
        }
        le32(cd, p + CDH_EXTERNAL_ATTR_OFS) & DOS_DIR_ATTRIBUTE_BITFLAG != 0
    }

    /// `mz_zip_reader_file_stat(pZip, file_index, pStat)` (`mz_zip_file_stat_internal`): `false`
    /// with the error set where the C fails, `stat` filled as far as the C fills it.
    pub fn reader_file_stat(&mut self, file_index: u32, stat: &mut FileStat) -> bool {
        let Some(p) = self.cdh(file_index) else {
            return self.set_error(ZipError::InvalidParameter);
        };
        let cd = self.cd();
        stat.m_file_index = file_index;
        stat.m_central_dir_ofs = p as u64;
        stat.m_version_made_by = le16(cd, p + CDH_VERSION_MADE_BY_OFS) as u16;
        stat.m_version_needed = le16(cd, p + CDH_VERSION_NEEDED_OFS) as u16;
        stat.m_bit_flag = le16(cd, p + CDH_BIT_FLAG_OFS) as u16;
        stat.m_method = le16(cd, p + CDH_METHOD_OFS) as u16;
        stat.m_crc32 = le32(cd, p + CDH_CRC32_OFS);
        stat.m_comp_size = u64::from(le32(cd, p + CDH_COMPRESSED_SIZE_OFS));
        stat.m_uncomp_size = u64::from(le32(cd, p + CDH_DECOMPRESSED_SIZE_OFS));
        stat.m_internal_attr = le16(cd, p + CDH_INTERNAL_ATTR_OFS) as u16;
        stat.m_external_attr = le32(cd, p + CDH_EXTERNAL_ATTR_OFS);
        stat.m_local_header_ofs = u64::from(le32(cd, p + CDH_LOCAL_HEADER_OFS));

        let name_len = le16(cd, p + CDH_FILENAME_LEN_OFS) as usize;
        let extra_len = le16(cd, p + CDH_EXTRA_LEN_OFS) as usize;
        let n = Ord::min(name_len, MZ_ZIP_MAX_ARCHIVE_FILENAME_SIZE - 1);
        let name = &cd[p + CENTRAL_DIR_HEADER_SIZE..p + CENTRAL_DIR_HEADER_SIZE + n];
        stat.m_filename = cstr(name).to_vec();
        let n = Ord::min(
            le16(cd, p + CDH_COMMENT_LEN_OFS) as usize,
            MZ_ZIP_MAX_ARCHIVE_FILE_COMMENT_SIZE - 1,
        );
        stat.m_comment_size = n as u32;
        let at = p + CENTRAL_DIR_HEADER_SIZE + name_len + extra_len;
        stat.m_comment = cstr(&cd[at..at + n]).to_vec();

        stat.m_is_directory = self.reader_is_file_a_directory(file_index);
        stat.m_is_encrypted = self.reader_is_file_encrypted(file_index);
        stat.m_is_supported = self.reader_is_file_supported(file_index);
        stat.m_is_utf8 = i32::from(stat.m_bit_flag & FLAG_UTF8);

        if umax(
            umax(stat.m_comp_size, stat.m_uncomp_size),
            stat.m_local_header_ofs,
        ) == MZ_UINT32_MAX
        {
            let cd = self.cd();
            let mut extra_size_remaining = extra_len;
            if extra_size_remaining != 0 {
                let mut at = p + CENTRAL_DIR_HEADER_SIZE + name_len;
                let mut err = None;
                loop {
                    if extra_size_remaining < 4 {
                        err = Some(ZipError::InvalidHeaderOrCorrupted);
                        break;
                    }
                    let field_id = le16(cd, at);
                    let field_data_size = le16(cd, at + 2) as usize;
                    if field_data_size + 4 > extra_size_remaining {
                        err = Some(ZipError::InvalidHeaderOrCorrupted);
                        break;
                    }
                    if field_id == ZIP64_EXTENDED_INFORMATION_FIELD_HEADER_ID {
                        let mut f = at + 4;
                        let mut remaining = field_data_size;
                        for which in 0..3 {
                            let target = match which {
                                0 => &mut stat.m_uncomp_size,
                                1 => &mut stat.m_comp_size,
                                _ => &mut stat.m_local_header_ofs,
                            };
                            if *target == MZ_UINT32_MAX {
                                if remaining < 8 {
                                    err = Some(ZipError::InvalidHeaderOrCorrupted);
                                    break;
                                }
                                *target = le64(cd, f);
                                f += 8;
                                remaining -= 8;
                            }
                        }
                        break;
                    }
                    at += 4 + field_data_size;
                    extra_size_remaining -= 4 + field_data_size;
                    if extra_size_remaining == 0 {
                        break;
                    }
                }
                if let Some(e) = err {
                    return self.set_error(e);
                }
            }
        }
        true
    }

    /// `mz_zip_reader_get_extra(pZip, file_index, pExtra, extra_buf_size)`: the entry's extra field
    /// copied into `out` (`extra_buf_size` is `out.len()`) as far as it fits with a NUL after it;
    /// returns the length copied plus one, or 0 for an entry that does not exist.
    pub fn reader_get_extra(&mut self, file_index: u32, out: &mut [u8]) -> u32 {
        let Some(p) = self.cdh(file_index) else {
            if !out.is_empty() {
                out[0] = 0;
            }
            self.set_error(ZipError::InvalidParameter);
            return 0;
        };
        let cd = self.cd();
        let nf = le16(cd, p + CDH_FILENAME_LEN_OFS) as usize;
        let mut ne = le16(cd, p + CDH_EXTRA_LEN_OFS) as usize;
        if !out.is_empty() {
            ne = Ord::min(ne, out.len() - 1);
            let at = p + CENTRAL_DIR_HEADER_SIZE + nf;
            out[..ne].copy_from_slice(&cd[at..at + ne]);
            out[ne] = 0;
        }
        (ne + 1) as u32
    }

    /// `mz_zip_locate_file_binary_search`.
    fn locate_file_binary_search(&mut self, name: &[u8]) -> Option<u32> {
        let size = self.m_total_files;
        if size != 0 {
            let st = self.state.as_ref()?;
            let (mut l, mut h) = (0i64, i64::from(size) - 1);
            while l <= h {
                let m = l + ((h - l) >> 1);
                let file_index = st.sorted_central_dir_offsets[m as usize];
                let comp =
                    filename_compare(&st.central_dir, &st.central_dir_offsets, file_index, name);
                if comp == 0 {
                    return Some(file_index);
                } else if comp < 0 {
                    l = m + 1;
                } else {
                    h = m - 1;
                }
            }
        }
        self.set_error(ZipError::FileNotFound);
        None
    }

    /// `mz_zip_reader_locate_file_v2(pZip, pName, pComment, flags, &index)`: the entry's index,
    /// or `None` with the error set (`pName` and `pComment` read as C strings).
    pub fn reader_locate_file_v2(
        &mut self,
        name: &[u8],
        comment: Option<&[u8]>,
        flags: u32,
    ) -> Option<u32> {
        let name = cstr(name);
        let Some(st) = self.state.as_ref() else {
            self.set_error(ZipError::InvalidParameter);
            return None;
        };
        if (st.init_flags & MZ_ZIP_FLAG_DO_NOT_SORT_CENTRAL_DIRECTORY) == 0
            && self.m_zip_mode == ZipMode::Reading
            && (flags & (MZ_ZIP_FLAG_IGNORE_PATH | MZ_ZIP_FLAG_CASE_SENSITIVE)) == 0
            && comment.is_none()
            && !st.sorted_central_dir_offsets.is_empty()
        {
            return self.locate_file_binary_search(name);
        }
        let name_len = name.len();
        if name_len as u64 > MZ_UINT16_MAX {
            self.set_error(ZipError::InvalidParameter);
            return None;
        }
        let comment = comment.map(cstr);
        let comment_len = comment.map_or(0, <[u8]>::len);
        if comment_len as u64 > MZ_UINT16_MAX {
            self.set_error(ZipError::InvalidParameter);
            return None;
        }
        for file_index in 0..self.m_total_files {
            let p = st.central_dir_offsets[file_index as usize] as usize;
            let cd = &st.central_dir;
            let mut filename_len = le16(cd, p + CDH_FILENAME_LEN_OFS) as usize;
            let mut fname = p + CENTRAL_DIR_HEADER_SIZE;
            if filename_len < name_len {
                continue;
            }
            if comment_len != 0 {
                let file_extra_len = le16(cd, p + CDH_EXTRA_LEN_OFS) as usize;
                let file_comment_len = le16(cd, p + CDH_COMMENT_LEN_OFS) as usize;
                let at = fname + filename_len + file_extra_len;
                let c = comment.unwrap_or_default();
                if file_comment_len != comment_len
                    || !string_equal(c, &cd[at..at + file_comment_len], flags)
                {
                    continue;
                }
            }
            if (flags & MZ_ZIP_FLAG_IGNORE_PATH) != 0 && filename_len != 0 {
                let mut ofs = filename_len as i64 - 1;
                loop {
                    let c = cd[fname + ofs as usize];
                    if c == b'/' || c == b'\\' || c == b':' {
                        break;
                    }
                    ofs -= 1;
                    if ofs < 0 {
                        break;
                    }
                }
                let ofs = (ofs + 1) as usize;
                fname += ofs;
                filename_len -= ofs;
            }
            if filename_len == name_len
                && string_equal(name, &cd[fname..fname + filename_len], flags)
            {
                return Some(file_index);
            }
        }
        self.set_error(ZipError::FileNotFound);
        None
    }

    /// `mz_zip_reader_locate_file(pZip, pName, pComment, flags)`: the index, or -1.
    pub fn reader_locate_file(&mut self, name: &[u8], comment: Option<&[u8]>, flags: u32) -> i32 {
        self.reader_locate_file_v2(name, comment, flags)
            .map_or(-1, |i| i as i32)
    }

    /// The checks an extraction starts with, through the local header: the entry's statistics
    /// and where its data starts, or `Err(result)` with the C's early return.
    fn begin_extract(&mut self, file_index: u32, flags: u32) -> Result<FileStat, bool> {
        let mut st = FileStat::default();
        if !self.reader_file_stat(file_index, &mut st) {
            return Err(false);
        }
        if st.m_is_directory || st.m_comp_size == 0 {
            return Err(true);
        }
        if st.m_bit_flag
            & (FLAG_IS_ENCRYPTED | FLAG_USES_STRONG_ENCRYPTION | FLAG_COMPRESSED_PATCH_FLAG)
            != 0
        {
            return Err(self.set_error(ZipError::UnsupportedEncryption));
        }
        if (flags & MZ_ZIP_FLAG_COMPRESSED_DATA) == 0
            && st.m_method != 0
            && st.m_method != MZ_DEFLATED
        {
            return Err(self.set_error(ZipError::UnsupportedMethod));
        }
        Ok(st)
    }

    /// The local header: where the entry's data starts, or `Err(false)` with the error set.
    fn local_data_ofs(&mut self, st: &FileStat) -> Result<u64, bool> {
        let mut cur_file_ofs = st.m_local_header_ofs;
        let mut lh = [0u8; LOCAL_DIR_HEADER_SIZE];
        if self.read(cur_file_ofs, &mut lh) != LOCAL_DIR_HEADER_SIZE {
            return Err(self.set_error(ZipError::FileReadFailed));
        }
        if le32(&lh, 0) != LOCAL_DIR_HEADER_SIG {
            return Err(self.set_error(ZipError::InvalidHeaderOrCorrupted));
        }
        cur_file_ofs = cur_file_ofs.wrapping_add(
            (LOCAL_DIR_HEADER_SIZE as u32
                + le16(&lh, LDH_FILENAME_LEN_OFS)
                + le16(&lh, LDH_EXTRA_LEN_OFS))
            .into(),
        );
        if cur_file_ofs.wrapping_add(st.m_comp_size) > self.m_archive_size {
            return Err(self.set_error(ZipError::InvalidHeaderOrCorrupted));
        }
        Ok(cur_file_ofs)
    }

    /// `mz_zip_reader_extract_to_mem(pZip, file_index, pBuf, buf_size, flags)`
    /// (`mz_zip_reader_extract_to_mem_no_alloc` without a caller's read buffer): `buf_size` is
    /// `buf.len()`.
    pub fn reader_extract_to_mem(&mut self, file_index: u32, buf: &mut [u8], flags: u32) -> bool {
        if self.state.is_none() || !self.has_read() {
            return self.set_error(ZipError::InvalidParameter);
        }
        let st = match self.begin_extract(file_index, flags) {
            Ok(st) => st,
            Err(r) => return r,
        };
        let needed_size = if flags & MZ_ZIP_FLAG_COMPRESSED_DATA != 0 {
            st.m_comp_size
        } else {
            st.m_uncomp_size
        };
        if (buf.len() as u64) < needed_size {
            return self.set_error(ZipError::BufTooSmall);
        }
        let mut cur_file_ofs = match self.local_data_ofs(&st) {
            Ok(o) => o,
            Err(r) => return r,
        };
        if (flags & MZ_ZIP_FLAG_COMPRESSED_DATA) != 0 || st.m_method == 0 {
            let needed = needed_size as usize;
            if self.read(cur_file_ofs, &mut buf[..needed]) != needed {
                return self.set_error(ZipError::FileReadFailed);
            }
            if (flags & MZ_ZIP_FLAG_COMPRESSED_DATA) == 0
                && mz_crc32(MZ_CRC32_INIT, &buf[..st.m_uncomp_size as usize]) != st.m_crc32
            {
                return self.set_error(ZipError::CrcCheckFailed);
            }
            return true;
        }

        let mut inflator = Decompressor::new();
        let (mut input, read_buf_size, mut read_buf_avail, mut comp_remaining) = if self.is_mem() {
            (
                Input::Mem {
                    ofs: cur_file_ofs as usize,
                },
                st.m_comp_size,
                st.m_comp_size,
                0u64,
            )
        } else {
            let size = Ord::min(st.m_comp_size, MZ_ZIP_MAX_IO_BUF_SIZE);
            (
                Input::Buf(vec![0u8; size as usize]),
                size,
                0u64,
                st.m_comp_size,
            )
        };
        let mut read_buf_ofs = 0u64;
        let mut out_buf_ofs = 0u64;
        let mut status;
        loop {
            let out_buf_size = (st.m_uncomp_size - out_buf_ofs) as usize;
            if read_buf_avail == 0 && !self.is_mem() {
                read_buf_avail = Ord::min(read_buf_size, comp_remaining);
                let Input::Buf(b) = &mut input else {
                    unreachable!("a read buffer outside memory")
                };
                let mut tmp = std::mem::take(b);
                let got = self.read(cur_file_ofs, &mut tmp[..read_buf_avail as usize]);
                if let Input::Buf(b) = &mut input {
                    *b = tmp;
                }
                if got != read_buf_avail as usize {
                    status = Status::Failed;
                    self.set_error(ZipError::DecompressionFailed);
                    break;
                }
                cur_file_ofs += read_buf_avail;
                comp_remaining -= read_buf_avail;
                read_buf_ofs = 0;
            }
            let in_slice: &[u8] = match (&input, &self.source) {
                (Input::Mem { ofs }, Source::Mem(mem)) => {
                    let at = *ofs + read_buf_ofs as usize;
                    &mem[at..at + read_buf_avail as usize]
                }
                (Input::Buf(b), _) => {
                    let at = read_buf_ofs as usize;
                    &b[at..at + read_buf_avail as usize]
                }
                (Input::Mem { .. }, _) => &[],
            };
            let tflags = TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF
                | if comp_remaining != 0 {
                    TINFL_FLAG_HAS_MORE_INPUT
                } else {
                    0
                };
            let pr = inflator.decompress(in_slice, buf, out_buf_ofs as usize, out_buf_size, tflags);
            status = pr.status;
            read_buf_avail -= pr.in_consumed as u64;
            read_buf_ofs += pr.in_consumed as u64;
            out_buf_ofs += pr.out_written as u64;
            if status != Status::NeedsMoreInput {
                break;
            }
        }
        if status == Status::Done {
            if out_buf_ofs != st.m_uncomp_size {
                self.set_error(ZipError::UnexpectedDecompressedSize);
                status = Status::Failed;
            } else if mz_crc32(MZ_CRC32_INIT, &buf[..st.m_uncomp_size as usize]) != st.m_crc32 {
                self.set_error(ZipError::CrcCheckFailed);
                status = Status::Failed;
            }
        }
        status == Status::Done
    }

    /// `mz_zip_reader_extract_file_to_mem(pZip, pFilename, pBuf, buf_size, flags)`.
    pub fn reader_extract_file_to_mem(
        &mut self,
        filename: &[u8],
        buf: &mut [u8],
        flags: u32,
    ) -> bool {
        let Some(file_index) = self.reader_locate_file_v2(filename, None, flags) else {
            return false;
        };
        self.reader_extract_to_mem(file_index, buf, flags)
    }

    /// `mz_zip_reader_extract_to_callback(pZip, file_index, pCallback, pOpaque, flags)`: the
    /// callback gets each chunk's offset and bytes and returns how many it took; an `Err` from it
    /// ends the extraction there and comes back as it is (the C++ callback's exception).
    #[allow(clippy::too_many_lines)]
    pub fn reader_extract_to_callback<E>(
        &mut self,
        file_index: u32,
        callback: &mut WriteFunc<'_, E>,
        flags: u32,
    ) -> Result<bool, E> {
        let mut status = Status::Done;
        let mut file_crc32 = MZ_CRC32_INIT;
        if self.state.is_none() || !self.has_read() {
            return Ok(self.set_error(ZipError::InvalidParameter));
        }
        let st = match self.begin_extract(file_index, flags) {
            Ok(st) => st,
            Err(r) => return Ok(r),
        };
        let mut cur_file_ofs = match self.local_data_ofs(&st) {
            Ok(o) => o,
            Err(r) => return Ok(r),
        };
        let mem = self.is_mem();
        let (mut input, read_buf_size, mut read_buf_avail, mut comp_remaining) = if mem {
            (
                Input::Mem {
                    ofs: cur_file_ofs as usize,
                },
                st.m_comp_size,
                st.m_comp_size,
                0u64,
            )
        } else {
            let size = Ord::min(st.m_comp_size, MZ_ZIP_MAX_IO_BUF_SIZE);
            (
                Input::Buf(vec![0u8; size as usize]),
                size,
                0u64,
                st.m_comp_size,
            )
        };
        let mut read_buf_ofs = 0u64;
        let mut out_buf_ofs = 0u64;

        if (flags & MZ_ZIP_FLAG_COMPRESSED_DATA) != 0 || st.m_method == 0 {
            if mem {
                let Source::Mem(m) = &self.source else {
                    unreachable!("memory without its bytes")
                };
                let at = cur_file_ofs as usize;
                let chunk = &m[at..at + st.m_comp_size as usize];
                if callback(out_buf_ofs, chunk)? as u64 != st.m_comp_size {
                    self.set_error(ZipError::WriteCallbackFailed);
                    status = Status::Failed;
                } else if (flags & MZ_ZIP_FLAG_COMPRESSED_DATA) == 0 {
                    file_crc32 = mz_crc32(file_crc32, chunk);
                }
                out_buf_ofs += st.m_comp_size;
            } else {
                let Input::Buf(mut b) = input else {
                    unreachable!("a read buffer outside memory")
                };
                while comp_remaining != 0 {
                    read_buf_avail = Ord::min(read_buf_size, comp_remaining);
                    let n = read_buf_avail as usize;
                    if self.read(cur_file_ofs, &mut b[..n]) != n {
                        self.set_error(ZipError::FileReadFailed);
                        status = Status::Failed;
                        break;
                    }
                    if (flags & MZ_ZIP_FLAG_COMPRESSED_DATA) == 0 {
                        file_crc32 = mz_crc32(file_crc32, &b[..n]);
                    }
                    if callback(out_buf_ofs, &b[..n])? != n {
                        self.set_error(ZipError::WriteCallbackFailed);
                        status = Status::Failed;
                        break;
                    }
                    cur_file_ofs += read_buf_avail;
                    out_buf_ofs += read_buf_avail;
                    comp_remaining -= read_buf_avail;
                }
            }
        } else {
            let mut inflator = Decompressor::new();
            let mut write_buf = vec![0u8; TINFL_LZ_DICT_SIZE];
            loop {
                let cur = (out_buf_ofs & (TINFL_LZ_DICT_SIZE as u64 - 1)) as usize;
                let out_buf_size = TINFL_LZ_DICT_SIZE - cur;
                if read_buf_avail == 0 && !mem {
                    read_buf_avail = Ord::min(read_buf_size, comp_remaining);
                    let Input::Buf(b) = &mut input else {
                        unreachable!("a read buffer outside memory")
                    };
                    let mut tmp = std::mem::take(b);
                    let got = self.read(cur_file_ofs, &mut tmp[..read_buf_avail as usize]);
                    if let Input::Buf(b) = &mut input {
                        *b = tmp;
                    }
                    if got != read_buf_avail as usize {
                        self.set_error(ZipError::FileReadFailed);
                        status = Status::Failed;
                        break;
                    }
                    cur_file_ofs += read_buf_avail;
                    comp_remaining -= read_buf_avail;
                    read_buf_ofs = 0;
                }
                let in_slice: &[u8] = match (&input, &self.source) {
                    (Input::Mem { ofs }, Source::Mem(m)) => {
                        let at = *ofs + read_buf_ofs as usize;
                        &m[at..at + read_buf_avail as usize]
                    }
                    (Input::Buf(b), _) => {
                        let at = read_buf_ofs as usize;
                        &b[at..at + read_buf_avail as usize]
                    }
                    (Input::Mem { .. }, _) => &[],
                };
                let tflags = if comp_remaining != 0 {
                    TINFL_FLAG_HAS_MORE_INPUT
                } else {
                    0
                };
                let pr = inflator.decompress(in_slice, &mut write_buf, cur, out_buf_size, tflags);
                status = pr.status;
                read_buf_avail -= pr.in_consumed as u64;
                read_buf_ofs += pr.in_consumed as u64;
                if pr.out_written != 0 {
                    let chunk = &write_buf[cur..cur + pr.out_written];
                    if callback(out_buf_ofs, chunk)? != pr.out_written {
                        self.set_error(ZipError::WriteCallbackFailed);
                        status = Status::Failed;
                        break;
                    }
                    file_crc32 = mz_crc32(file_crc32, chunk);
                    out_buf_ofs += pr.out_written as u64;
                    if out_buf_ofs > st.m_uncomp_size {
                        self.set_error(ZipError::DecompressionFailed);
                        status = Status::Failed;
                        break;
                    }
                }
                if status != Status::NeedsMoreInput && status != Status::HasMoreOutput {
                    break;
                }
            }
        }

        if status == Status::Done && (flags & MZ_ZIP_FLAG_COMPRESSED_DATA) == 0 {
            if out_buf_ofs != st.m_uncomp_size {
                self.set_error(ZipError::UnexpectedDecompressedSize);
                status = Status::Failed;
            } else if file_crc32 != st.m_crc32 {
                self.set_error(ZipError::DecompressionFailed);
                status = Status::Failed;
            }
        }
        Ok(status == Status::Done)
    }

    /// `mz_zip_reader_extract_file_to_callback(pZip, pFilename, pCallback, pOpaque, flags)`.
    pub fn reader_extract_file_to_callback<E>(
        &mut self,
        filename: &[u8],
        callback: &mut WriteFunc<'_, E>,
        flags: u32,
    ) -> Result<bool, E> {
        let Some(file_index) = self.reader_locate_file_v2(filename, None, flags) else {
            return Ok(false);
        };
        self.reader_extract_to_callback(file_index, callback, flags)
    }

    /// `mz_zip_reader_extract_to_file(pZip, file_index, pDst_filename, flags)`: the entry written
    /// to a new file (opened as `fopen(path, "wb")` opens it), its time not set (`m_time`).
    pub fn reader_extract_to_file(
        &mut self,
        file_index: u32,
        dst: &std::path::Path,
        flags: u32,
    ) -> bool {
        let mut st = FileStat::default();
        if !self.reader_file_stat(file_index, &mut st) {
            return false;
        }
        if st.m_is_directory || !st.m_is_supported {
            return self.set_error(ZipError::UnsupportedFeature);
        }
        let Ok(mut file) = File::create(dst) else {
            return self.set_error(ZipError::FileOpenFailed);
        };
        // mz_zip_file_write_callback: fwrite, which takes every byte unless the write fails.
        let mut write = |_ofs: u64, b: &[u8]| -> Result<usize, std::convert::Infallible> {
            Ok(if file.write_all(b).is_ok() {
                b.len()
            } else {
                0
            })
        };
        let Ok(status) = self.reader_extract_to_callback(file_index, &mut write, flags);
        status
    }
}

/// `mz_zip_reader_filename_less`: entry `l_index`'s name before entry `r_index`'s, without case.
fn filename_less(cd: &[u8], offs: &[u32], l_index: u32, r_index: u32) -> bool {
    let pl = offs[l_index as usize] as usize;
    let pr = offs[r_index as usize] as usize;
    let l_len = le16(cd, pl + CDH_FILENAME_LEN_OFS) as usize;
    let r_len = le16(cd, pr + CDH_FILENAME_LEN_OFS) as usize;
    let (mut a, mut b) = (pl + CENTRAL_DIR_HEADER_SIZE, pr + CENTRAL_DIR_HEADER_SIZE);
    let e = a + Ord::min(l_len, r_len);
    let (mut l, mut r) = (0u8, 0u8);
    while a < e {
        l = tolower(cd[a]);
        r = tolower(cd[b]);
        if l != r {
            break;
        }
        a += 1;
        b += 1;
    }
    if a == e { l_len < r_len } else { l < r }
}

/// `mz_zip_filename_compare`: entry `l_index`'s name against `name`, without case.
fn filename_compare(cd: &[u8], offs: &[u32], l_index: u32, name: &[u8]) -> i32 {
    let pl = offs[l_index as usize] as usize;
    let l_len = le16(cd, pl + CDH_FILENAME_LEN_OFS);
    let r_len = name.len() as u32;
    let mut a = pl + CENTRAL_DIR_HEADER_SIZE;
    let mut b = 0usize;
    let e = a + Ord::min(l_len, r_len) as usize;
    let (mut l, mut r) = (0u8, 0u8);
    while a < e {
        l = tolower(cd[a]);
        r = tolower(name[b]);
        if l != r {
            break;
        }
        a += 1;
        b += 1;
    }
    if a == e {
        l_len.wrapping_sub(r_len) as i32
    } else {
        i32::from(l) - i32::from(r)
    }
}

/// `mz_zip_string_equal(pA, pB, len, flags)`: the first `b.len()` bytes alike, without case unless
/// `MZ_ZIP_FLAG_CASE_SENSITIVE`.
fn string_equal(a: &[u8], b: &[u8], flags: u32) -> bool {
    let len = b.len();
    if a.len() < len {
        // pA is a C string shorter than len: its NUL differs from b's byte there.
        return false;
    }
    if flags & MZ_ZIP_FLAG_CASE_SENSITIVE != 0 {
        return a[..len] == b[..len];
    }
    (0..len).all(|i| tolower(a[i]) == tolower(b[i]))
}
