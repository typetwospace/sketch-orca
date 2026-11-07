// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries ADMesh code: deps_src/admesh/stlinit.cpp of OrcaSlicer v2.4.2 (stl_open,
// stl_open_count_facets, stl_read, stl_allocate, stl_reallocate, stl_facet_stats), translated on
// 2026-11-07. Copyright (C) 1995, 1996 Anthony D. Martin and (C) 2013, 2014 the ADMesh
// contributors, with changes by the PrusaSlicer and OrcaSlicer authors, under the GNU General
// Public License, version 2 or later (GPL-2.0-or-later; licenses/GPL-2.0.txt), taken here under
// version 3 (licenses/GPL-3.0.txt) as section 13 of the AGPL-3.0 allows.

//! `stl_open`: admesh's STL reader, through `orca-fmt`'s model of the static UCRT's stdio.
//!
//! How a file is read, as the C++ reads it:
//!
//! 1. Opened `"rb"`, its size taken with `fseek` to the end and `ftell`. The 128 bytes after the
//!    header (`custom_header_length + 4`, at least 84) decide the type: binary if any is above
//!    127, else ASCII. A file too short to hold them is refused as empty, ASCII ones included.
//! 2. Binary: the size less the header must be a whole number of 50-byte facets, and the file at
//!    least 284 bytes; the facet count is taken from the size (a different count in the header is
//!    only logged). ASCII: the file is opened again in text mode, and its lines counted with
//!    `fgets(buf, 100)`, each piece of at most 99 bytes counting unless it is 4 bytes or shorter or
//!    starts with `solid` or `endsolid`; the facet count is that count, plus one, over 7.
//! 3. The facets are read into a zeroed array: binary ones as they are; ASCII ones by the
//!    `fscanf` sequence below, where any part missing fails the whole file. A facet with a NaN
//!    coordinate is skipped, and stays zero (`stl_check_facets_exact` removes it as degenerate).
//!    The bounding box and the first facet's longest edge coordinate (`shortest_edge`, made
//!    shorter by the edge matching) come from the facets kept.
//!
//! The ASCII facet: `fscanf(" endsolid%*[^\n]\n")`, `fscanf(" solid%*[^\n]\n")`, then
//! `" facet normal %31s %31s %31s"` (3), `" outer loop"` (0: a literal that does not match fails
//! nothing), `" vertex %f %f %f"` twice and `" vertex %f %f %f "` (3 each), a line that starts
//! with `endloop` and then CR, LF, space or tab (`fgets(buf, 2047)`), `fscanf(" ")`, and a line
//! that starts with `endfacet` and one of those four. The normal's three words are each read with
//! `sscanf("%f")`; if any fails the normal is zero.
//!
//! One difference, a defined answer where the C++ has none: before the facets the reader scans
//! `" solid %[^\n]"` into `char solid_name[256]`, and a `solid` line longer than 255 bytes (a file
//! with CR line ends has a single line) overruns that buffer in the C++ engine. Here
//! [`stl_open`] stops there and returns [`StlError::OverlongSolidLine`] (`parity/CORPUS.md`,
//! "20mmbox-CR.stl"; `parity/HAZARDS.md`, the CR box). What the C++ does with the name after it
//! (the `MW` version, model id and country code it reads with `sscanf("%s %s %s")` into
//! `char[16]`, `char[128]` and `char[16]`) reaches only the progress callback, which the engine
//! does not pass, so it is not done here.

use std::fmt;
use std::path::Path;

use orca_fmt::scanf::{ScanArg, fscanf, sscanf};
use orca_fmt::stdio::{File, Mode, SEEK_END, SEEK_SET};

use super::{
    ASCII_LINES_PER_FACET, LABEL_SIZE, NUM_FACET_SIZE, SIZEOF_STL_FACET, STL_MIN_FILE_SIZE,
    StlFacet, StlFile, StlNeighbors, StlType, max3,
};
use crate::log::{Severity, log};

/// Why `stl_open` returned false, or the overlong `solid` line it refuses (see the module
/// documentation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StlError {
    /// `fopen` failed: "stl_open_count_facets: Couldn't open <file> for reading".
    Open,
    /// The 128 bytes after the header could not be read: "The input is an empty file".
    Empty,
    /// A binary file whose size is not the header and whole facets, or below 284 bytes.
    WrongSize,
    /// A binary facet could not be read whole.
    ShortRead,
    /// An ASCII facet that does not parse: "Something is syntactically very wrong with this ASCII
    /// STL!".
    Syntax,
    /// An ASCII file whose `solid` line is longer than the 255 bytes the C++ reader's buffer
    /// holds: `length` is how many bytes of it were read before the reader stopped (256).
    OverlongSolidLine {
        /// The bytes of the line read before stopping.
        length: usize,
    },
}

impl fmt::Display for StlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StlError::Open => f.write_str("the file cannot be opened for reading"),
            StlError::Empty => f.write_str("the file is too short to tell binary from ASCII STL"),
            StlError::WrongSize => {
                f.write_str("the binary STL's size is not its header and whole 50-byte facets")
            }
            StlError::ShortRead => f.write_str("a binary STL facet could not be read whole"),
            StlError::Syntax => f.write_str("the ASCII STL is not syntactically valid"),
            StlError::OverlongSolidLine { length } => write!(
                f,
                "the ASCII STL's solid line is longer than 255 bytes (at least {length} read): \
                 admesh reads it into a 256-byte buffer, which such a line overruns (a file with \
                 CR line ends is one line)"
            ),
        }
    }
}

impl std::error::Error for StlError {}

/// `strlen` of a C string held in `buf`.
fn c_strlen(buf: &[u8]) -> usize {
    buf.iter().position(|&c| c == 0).unwrap_or(buf.len())
}

/// `strncmp(buf, prefix, prefix.len()) == 0` for a C string `buf`.
fn c_starts_with(buf: &[u8], prefix: &[u8]) -> bool {
    let len = c_strlen(buf);
    len >= prefix.len() && &buf[..prefix.len()] == prefix
}

/// `stl_open(stl, file, nullptr, custom_header_length)`: reads the STL file at `path` into `stl`
/// (cleared first), naming the file `file` in the log, as the C++ does with the path it is given.
///
/// # Errors
///
/// The reasons `stl_open` returns false, logged as it logs them, and the overlong `solid` line of
/// the module documentation.
pub fn stl_open(
    stl: &mut StlFile,
    path: &Path,
    file: &str,
    custom_header_length: i32,
) -> Result<(), StlError> {
    let raw = std::fs::read(path).ok();
    stl_open_bytes(stl, raw.as_deref(), file, custom_header_length)
}

/// [`stl_open`] of a file that holds `raw` (`None`: a file `fopen` cannot open).
///
/// # Errors
///
/// As [`stl_open`].
pub fn stl_open_bytes(
    stl: &mut StlFile,
    raw: Option<&[u8]>,
    file: &str,
    custom_header_length: i32,
) -> Result<(), StlError> {
    let custom_header_length = if custom_header_length < LABEL_SIZE {
        LABEL_SIZE
    } else {
        custom_header_length
    };
    stl.clear();
    stl.stats.reset_header(custom_header_length);
    let mut fp = stl_open_count_facets(stl, raw, file, custom_header_length)?;
    stl_allocate(stl);
    stl_read(stl, &mut fp, 0, true, custom_header_length)
}

/// `stl_open_count_facets`: the type, the header and the facet count; the stream to read the
/// facets from.
fn stl_open_count_facets(
    stl: &mut StlFile,
    raw: Option<&[u8]>,
    file: &str,
    custom_header_length: i32,
) -> Result<File, StlError> {
    let Some(raw) = raw else {
        log(
            Severity::Error,
            &format!("stl_open_count_facets: Couldn't open {file} for reading"),
        );
        return Err(StlError::Open);
    };
    let mut fp = File::from_bytes(raw, Mode::Binary);
    fp.fseek(0, SEEK_END);
    let file_size = fp.ftell();

    let header_size = custom_header_length + NUM_FACET_SIZE;
    fp.fseek(header_size, SEEK_SET);
    let mut chtest = [0u8; 128];
    if fp.fread(&mut chtest, 128, 1) == 0 {
        log(
            Severity::Error,
            &format!("stl_open_count_facets: The input is an empty file: {file}"),
        );
        return Err(StlError::Empty);
    }
    stl.stats.type_ = StlType::Ascii;
    if chtest.iter().any(|&c| c > 127) {
        stl.stats.type_ = StlType::Binary;
    }
    fp.rewind();

    let chl = custom_header_length as usize;
    let num_facets: u32;
    if stl.stats.type_ == StlType::Binary {
        if (file_size - header_size) % SIZEOF_STL_FACET as i32 != 0 || file_size < STL_MIN_FILE_SIZE
        {
            log(
                Severity::Error,
                &format!("stl_open_count_facets: The file {file} has the wrong size."),
            );
            return Err(StlError::WrongSize);
        }
        num_facets = ((file_size - header_size) / SIZEOF_STL_FACET as i32) as u32;

        // fread(header, custom_header_length, 1) returns 0 or 1, never above
        // custom_header_length - 1, so the C++ never writes the NUL it means to (the header is
        // zeroed already).
        fp.fread(&mut stl.stats.header[..chl], chl, 1);

        let mut count = [0u8; 4];
        let header_num_faces_read = fp.fread(&mut count, 4, 1) != 0;
        let header_num_facets = u32::from_le_bytes(count);
        if !header_num_faces_read || num_facets != header_num_facets {
            log(
                Severity::Info,
                &format!(
                    "stl_open_count_facets: Warning: File size doesn't match number of facets in \
                     the header: {file}"
                ),
            );
        }
    } else {
        // freopen(file, "r", fp): the same bytes, now through the text mode.
        fp = File::from_bytes(raw, Mode::Text);

        let mut linebuf = [0u8; 100];
        let mut num_lines: i32 = 1;
        while fp.fgets(&mut linebuf, 100) {
            if c_strlen(&linebuf) <= 4 {
                continue;
            }
            if c_starts_with(&linebuf, b"solid") || c_starts_with(&linebuf, b"endsolid") {
                continue;
            }
            num_lines += 1;
        }

        fp.rewind();

        // The header: getc up to custom_header_length bytes or a '\n', EOF stored as (char)-1.
        let mut i = 0usize;
        while i < chl {
            let c = fp.getc() as u8;
            stl.stats.header[i] = c;
            if c == b'\n' {
                break;
            }
            i += 1;
        }
        stl.stats.header[i] = 0;
        stl.stats.header[chl] = 0;

        num_facets = (num_lines / ASCII_LINES_PER_FACET) as u32;
    }

    stl.stats.number_of_facets = stl.stats.number_of_facets.wrapping_add(num_facets);
    stl.stats.original_num_facets = stl.stats.number_of_facets as i32;
    Ok(fp)
}

/// Whether a line read by `fgets` starts with `word` and then CR, LF, space or tab; a failed
/// `fgets` leaves the C buffer as it was, which holds no such line (see the module documentation).
fn line_is(read: bool, buf: &[u8], word: &[u8]) -> bool {
    read && c_starts_with(buf, word) && matches!(buf[word.len()], b'\r' | b'\n' | b' ' | b'\t')
}

/// `stl_read`: the facets from `first_facet` on (see the module documentation).
fn stl_read(
    stl: &mut StlFile,
    fp: &mut File,
    first_facet: u32,
    mut first: bool,
    custom_header_length: i32,
) -> Result<(), StlError> {
    if stl.stats.type_ == StlType::Binary {
        fp.fseek(custom_header_length + NUM_FACET_SIZE, SEEK_SET);
    } else {
        fp.rewind();
        let mut solid_name = [0u8; 256];
        if let Err(overflow) = fscanf(fp, b" solid %[^\n]", &mut [ScanArg::Chars(&mut solid_name)])
        {
            return Err(StlError::OverlongSolidLine {
                length: overflow.size,
            });
        }
        fp.rewind();
    }

    let mut normal_buf = [[0u8; 32]; 3];
    let mut buf = [0u8; 2048];
    let facets_num = stl.stats.number_of_facets;
    for i in first_facet..facets_num {
        let mut facet = StlFacet::ZERO;
        if stl.stats.type_ == StlType::Binary {
            let mut bytes = [0u8; SIZEOF_STL_FACET];
            if fp.fread(&mut bytes, 1, SIZEOF_STL_FACET) != SIZEOF_STL_FACET {
                return Err(StlError::ShortRead);
            }
            facet = StlFacet::from_bytes(&bytes);
        } else {
            let (mut res_vertex1, mut res_vertex2, mut res_vertex3) = (0, 0, 0);
            // No format here can overrun a buffer: the only strings stored are the normal's,
            // each at most 31 bytes (`%31s`) into 32.
            let scan = |fp: &mut File, format: &[u8], args: &mut [ScanArg<'_>]| -> i32 {
                fscanf(fp, format, args).unwrap_or_else(|_| unreachable!("no buffer to overrun"))
            };
            scan(fp, b" endsolid%*[^\n]\n", &mut []);
            scan(fp, b" solid%*[^\n]\n", &mut []);
            let res_normal = {
                let [n0, n1, n2] = &mut normal_buf;
                scan(
                    fp,
                    b" facet normal %31s %31s %31s",
                    &mut [ScanArg::Chars(n0), ScanArg::Chars(n1), ScanArg::Chars(n2)],
                )
            };
            let res_outer_loop = scan(fp, b" outer loop", &mut []);
            for (k, format, res) in [
                (0usize, &b" vertex %f %f %f"[..], &mut res_vertex1),
                (1, &b" vertex %f %f %f"[..], &mut res_vertex2),
                (2, &b" vertex %f %f %f "[..], &mut res_vertex3),
            ] {
                let mut v = facet.vertex[k].0;
                let [x, y, z] = &mut v;
                *res = scan(
                    fp,
                    format,
                    &mut [ScanArg::F32(x), ScanArg::F32(y), ScanArg::F32(z)],
                );
                facet.vertex[k].0 = v;
            }
            let read = fp.fgets(&mut buf, 2047);
            let endloop_ok = line_is(read, &buf, b"endloop");
            scan(fp, b" ", &mut []);
            let read = fp.fgets(&mut buf, 2047);
            let endfacet_ok = line_is(read, &buf, b"endfacet");
            if res_normal != 3
                || res_outer_loop != 0
                || res_vertex1 != 3
                || res_vertex2 != 3
                || res_vertex3 != 3
                || !endloop_ok
                || !endfacet_ok
            {
                log(
                    Severity::Error,
                    "Something is syntactically very wrong with this ASCII STL! ",
                );
                return Err(StlError::Syntax);
            }

            let mut normal = [0f32; 3];
            let mut ok = true;
            for (k, word) in normal_buf.iter().enumerate() {
                let r = sscanf(word, b"%f", &mut [ScanArg::F32(&mut normal[k])])
                    .unwrap_or_else(|_| unreachable!("no buffer to overrun"));
                if r != 1 {
                    ok = false;
                    break;
                }
            }
            facet.normal.0 = if ok { normal } else { [0.0; 3] };
        }

        if facet.vertex.iter().any(|v| v.0.iter().any(|c| c.is_nan())) {
            continue;
        }

        stl.facet_start[i as usize] = facet;
        stl_facet_stats(stl, &facet, &mut first);
    }

    stl.stats.size = stl.stats.max - stl.stats.min;
    stl.stats.bounding_diameter = stl.stats.size.norm();
    Ok(())
}

/// `stl_allocate`: `number_of_facets` zeroed facets and as many empty neighbour entries.
pub fn stl_allocate(stl: &mut StlFile) {
    let n = stl.facets();
    stl.facet_start.clear();
    stl.facet_start.resize(n, StlFacet::ZERO);
    stl.neighbors_start.clear();
    stl.neighbors_start.resize(n, StlNeighbors::RESET);
}

/// `stl_reallocate`: both arrays resized to `number_of_facets`.
pub fn stl_reallocate(stl: &mut StlFile) {
    let n = stl.facets();
    stl.facet_start.resize(n, StlFacet::ZERO);
    stl.neighbors_start.resize(n, StlNeighbors::RESET);
}

/// `stl_facet_stats`: the bounding box, and on the first facet the largest coordinate difference
/// of its first edge as `shortest_edge`.
pub fn stl_facet_stats(stl: &mut StlFile, facet: &StlFacet, first: &mut bool) {
    if *first {
        stl.stats.min = facet.vertex[0];
        stl.stats.max = facet.vertex[0];
        let diff = (facet.vertex[1] - facet.vertex[0]).cwise_abs();
        stl.stats.shortest_edge = max3(diff);
        *first = false;
    }
    for v in &facet.vertex {
        stl.stats.min = stl.stats.min.cwise_min(v);
        stl.stats.max = stl.stats.max.cwise_max(v);
    }
}
