// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The static UCRT's `FILE` streams as the engine's model readers use them, written from the C
//! standard and from black-box tests of the UCRT (`tests/scanf.rs`, the `ucrt_` tests; the
//! `cstdio.sop1` probe records), never from the UCRT's source: `fopen` and `freopen` for reading
//! (through `boost::nowide::fopen`), `fread`, `fgets`, `getc`, `ungetc`, `ftell`, `fseek`,
//! `rewind`, `feof` and `fclose`, as admesh's STL reader (`deps_src/admesh/stlinit.cpp`) and the
//! OBJ reader (`src/libslic3r/Format/objparser.cpp`) call them. [`crate::scanf`]'s `fscanf` reads
//! through these streams. Filled by the o2-mesh task (2026-11-07).
//!
//! How a stream works here: [`fopen`] reads the whole file at once. A binary stream (`"rb"`) reads
//! its bytes as they are. A text stream (`"r"`, `"rt"`) reads what the UCRT's text mode gives,
//! worked out up front: each CR LF pair reads as one LF, a CR that is not followed by an LF
//! (alone, before another CR, at the end of the file, or before a Ctrl-Z) reads as itself, and the
//! first Ctrl-Z (0x1A) ends the stream for good. The UCRT reads a file in 4096-byte pieces and
//! looks one byte ahead when a piece ends with a CR, so a CR LF across a piece's end reads as LF
//! like any other (`ucrt_text_mode`).
//!
//! What is not here, because no reader calls it: writing; a text stream's `ftell` and `fseek`
//! other than to its start (both return -1, as for an error); pushing back with `ungetc` anything
//! but the byte just read, which is all `fscanf` pushes back (any other push-back fails with
//! `EOF`); and `ftell` past 2 GiB, which returns -1 here, as a `long` cannot hold the position
//! (`parity/HAZARDS.md`: not black-box tested, a file that large being in no test).
//!
//! C's `stdout` is here too: [`stdout_write`] is where the engine's `printf` output goes (admesh's
//! `stl_write_facet`), the process's standard output unless a sink is set ([`set_stdout_sink`]).

use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

/// `EOF`.
pub const EOF: i32 = -1;
/// `SEEK_SET`.
pub const SEEK_SET: i32 = 0;
/// `SEEK_CUR`.
pub const SEEK_CUR: i32 = 1;
/// `SEEK_END`.
pub const SEEK_END: i32 = 2;

/// Ctrl-Z, which ends a text stream.
const CTRL_Z: u8 = 0x1A;

/// How a stream was opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// `"rb"`: the bytes as they are.
    Binary,
    /// `"r"` or `"rt"`: the UCRT's text mode (see the module documentation).
    Text,
}

/// A `FILE` opened for reading.
#[derive(Clone, Debug)]
pub struct File {
    /// What reads see: the file's bytes, through the text translation for a text stream.
    data: Vec<u8>,
    /// The next byte to read; may lie past the end after an `fseek`.
    pos: usize,
    /// The end-of-file indicator.
    eof: bool,
    mode: Mode,
}

/// The bytes a text stream reads from a file holding `raw` (see the module documentation).
#[must_use]
pub fn text_translation(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let c = raw[i];
        if c == CTRL_Z {
            break;
        }
        if c == b'\r' && raw.get(i + 1) == Some(&b'\n') {
            out.push(b'\n');
            i += 2;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// The stream mode of an `fopen` mode string that opens for reading: `"r"`, `"rt"` or `"rb"`.
///
/// # Panics
///
/// On any other mode: the readers open their files with these three only.
fn mode_of(mode: &str) -> Mode {
    match mode {
        "r" | "rt" => Mode::Text,
        "rb" => Mode::Binary,
        _ => panic!("orca-fmt stdio: fopen mode {mode:?} is not supported (only r, rt and rb)"),
    }
}

/// `fopen(path, mode)` for reading (`boost::nowide::fopen`, which opens the UTF-8 path as
/// `_wfopen` does): `None` where the C function returns a null pointer (no such file, a folder,
/// no access).
///
/// # Panics
///
/// On a mode other than `"r"`, `"rt"` and `"rb"`.
#[must_use]
pub fn fopen(path: &Path, mode: &str) -> Option<File> {
    let mode = mode_of(mode);
    let raw = std::fs::read(path).ok()?;
    Some(File::from_bytes(&raw, mode))
}

/// `freopen(path, mode, stream)`: closes `stream` and opens `path` again.
///
/// # Panics
///
/// As [`fopen`].
#[must_use]
pub fn freopen(path: &Path, mode: &str, stream: File) -> Option<File> {
    drop(stream);
    fopen(path, mode)
}

impl File {
    /// A stream over a file that holds `raw`, opened in `mode`.
    #[must_use]
    pub fn from_bytes(raw: &[u8], mode: Mode) -> File {
        let data = match mode {
            Mode::Binary => raw.to_vec(),
            Mode::Text => text_translation(raw),
        };
        File {
            data,
            pos: 0,
            eof: false,
            mode,
        }
    }

    /// The mode the stream was opened in.
    #[must_use]
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// `getc`: the next byte, or [`EOF`] (setting the end-of-file indicator).
    pub fn getc(&mut self) -> i32 {
        if let Some(&c) = self.data.get(self.pos) {
            self.pos += 1;
            i32::from(c)
        } else {
            self.eof = true;
            EOF
        }
    }

    /// `ungetc(c)` of the byte just read: steps back over it and clears the end-of-file
    /// indicator, returning `c`. [`EOF`], or a byte other than the one just read, is not pushed
    /// back: the call returns [`EOF`] (see the module documentation).
    pub fn ungetc(&mut self, c: i32) -> i32 {
        if c == EOF || self.pos == 0 || self.pos > self.data.len() {
            return EOF;
        }
        if i32::from(self.data[self.pos - 1]) != c {
            return EOF;
        }
        self.pos -= 1;
        self.eof = false;
        c
    }

    /// `fgets(buf, n, stream)`: reads up to `n - 1` bytes, stopping after a `\n`, into `buf`, with
    /// a NUL after them, and returns true. At the end of the stream with nothing read it returns
    /// false (C's null pointer) and leaves `buf` as it was.
    ///
    /// # Panics
    ///
    /// When `n` is below 2 or `buf` is shorter than `n` bytes.
    pub fn fgets(&mut self, buf: &mut [u8], n: usize) -> bool {
        assert!(
            n >= 2 && buf.len() >= n,
            "fgets: n {n}, buffer {}",
            buf.len()
        );
        let mut len = 0;
        while len < n - 1 {
            let c = self.getc();
            if c == EOF {
                break;
            }
            buf[len] = c as u8;
            len += 1;
            if c == i32::from(b'\n') {
                break;
            }
        }
        if len == 0 {
            return false;
        }
        buf[len] = 0;
        true
    }

    /// `fread(buf, size, count, stream)`: reads up to `size * count` bytes into `buf` and returns
    /// the number of whole items read; a short read sets the end-of-file indicator.
    ///
    /// Only the bytes read are written. The UCRT, asked at a text stream's start for 512 bytes or
    /// more, reads the file straight into `buf` (a multiple of 512 bytes) and translates it
    /// there, so the file's raw bytes are left after what it returns (`cstdio.sop1` showed it).
    /// C leaves those bytes unspecified (C11 7.21.8.1) and no reader looks past the count `fread`
    /// returns (`objparser.cpp`), so they are not written here (`parity/HAZARDS.md`, orca-mesh).
    ///
    /// # Panics
    ///
    /// When `buf` is shorter than `size * count` bytes.
    pub fn fread(&mut self, buf: &mut [u8], size: usize, count: usize) -> usize {
        if size == 0 || count == 0 {
            return 0;
        }
        let want = size
            .checked_mul(count)
            .expect("fread: size * count overflows");
        assert!(buf.len() >= want, "fread: buffer {} < {want}", buf.len());
        // Past the end (after an fseek there) nothing is read and the position stays.
        let available = self.data.len().saturating_sub(self.pos);
        let got = if want < available { want } else { available };
        if got > 0 {
            buf[..got].copy_from_slice(&self.data[self.pos..self.pos + got]);
            self.pos += got;
        }
        if got < want {
            self.eof = true;
        }
        got / size
    }

    /// `fseek(stream, offset, whence)` (`offset` a `long`): 0, or -1 when the position would be
    /// negative (the stream is left as it was). A binary stream may be positioned past its end,
    /// where reads find the end. A text stream takes only `fseek(stream, 0, SEEK_SET)`. Success
    /// clears the end-of-file indicator.
    ///
    /// # Panics
    ///
    /// On a `whence` other than [`SEEK_SET`], [`SEEK_CUR`] and [`SEEK_END`].
    pub fn fseek(&mut self, offset: i32, whence: i32) -> i32 {
        let base = match whence {
            SEEK_SET => 0i64,
            SEEK_CUR => self.pos as i64,
            SEEK_END => self.data.len() as i64,
            _ => panic!("fseek: whence {whence}"),
        };
        if self.mode == Mode::Text && !(whence == SEEK_SET && offset == 0) {
            return -1;
        }
        let target = base + i64::from(offset);
        if target < 0 {
            return -1;
        }
        self.pos = target as usize;
        self.eof = false;
        0
    }

    /// `ftell(stream)` (a `long`): the position of a binary stream, or -1 when it does not fit a
    /// `long` or the stream is a text stream (see the module documentation).
    #[must_use]
    pub fn ftell(&self) -> i32 {
        if self.mode == Mode::Text {
            return -1;
        }
        i32::try_from(self.pos).unwrap_or(-1)
    }

    /// `rewind(stream)`: back to the start, the end-of-file indicator cleared.
    pub fn rewind(&mut self) {
        self.pos = 0;
        self.eof = false;
    }

    /// `feof(stream)`: the end-of-file indicator.
    #[must_use]
    pub fn feof(&self) -> bool {
        self.eof
    }

    /// The number of bytes a read of the whole stream from its start gives.
    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether a read from the stream's start finds the end at once.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// The sink of C's `stdout`, when one is set.
pub type StdoutSink = Box<dyn FnMut(&[u8]) + Send>;

static STDOUT_SINK: Mutex<Option<StdoutSink>> = Mutex::new(None);

/// Sets where [`stdout_write`] sends C's `stdout` (`None`: the process's standard output, as at
/// the start) and returns the sink set before.
pub fn set_stdout_sink(sink: Option<StdoutSink>) -> Option<StdoutSink> {
    let mut guard = STDOUT_SINK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    std::mem::replace(&mut *guard, sink)
}

/// Writes `bytes` to C's `stdout`: what the engine's `printf` prints.
pub fn stdout_write(bytes: &[u8]) {
    let mut guard = STDOUT_SINK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(sink) = guard.as_mut() {
        sink(bytes);
    } else {
        let mut out = std::io::stdout().lock();
        // C's printf ignores a failed write too: admesh does not check what printf returns.
        let _ = out.write_all(bytes);
        let _ = out.flush();
    }
}
