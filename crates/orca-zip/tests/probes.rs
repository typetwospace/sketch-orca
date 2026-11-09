// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-zip: the records of `zip.sop1` (`parity/SOP1.md`, "Probe files"), from
//! the committed subset (`parity/probes/`), or from the full set when `SKETCH_ORCA_PROBES_DEEP`
//! names its folder (`<work>/parity/probes`).
//!
//! Each record is rebuilt here byte for byte as `parity/oracle/probe/zip.cpp` writes it, from the
//! inputs it holds and what this crate computes with them, and the two are compared whole; a
//! difference is named by the field it falls in. The kinds: ZCRC (`mz_crc32`), ZINF
//! (`tinfl_decompress` driven call by call), ZARC (an archive opened, its entries' statistics,
//! extra fields and data, names located, the archive closed) and ZERR (the error texts). Filled
//! by o2-model (2026-11-08).

use std::path::{Path, PathBuf};

use orca_probe::{Mismatches, ProbeFile, Reader, Record, load_probe};
use orca_zip::crc::mz_crc32;
use orca_zip::error::{ZipError, get_errorstr, mz_zip_get_error_string};
use orca_zip::extension::{MzArchive, close_zip_reader, open_zip_reader};
use orca_zip::reader::FileStat;
use orca_zip::tinfl::{
    Decompressor, Status, TINFL_FLAG_HAS_MORE_INPUT, TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
};

/// `MZ_ZIP_FLAG_COMPRESSED_DATA`.
const MZ_ZIP_FLAG_COMPRESSED_DATA: u32 = 0x0400;
/// The largest entry the probe extracts.
const MAX_EXTRACT: u64 = 8 << 20;

fn zip_file() -> ProbeFile {
    load_probe("zip").unwrap_or_else(|e| panic!("{e}"))
}

/// Bytes as `probe.hpp`'s `Payload` writes them, with where each field starts, to name a
/// difference.
#[derive(Default)]
struct Payload {
    bytes: Vec<u8>,
    fields: Vec<(usize, String)>,
}

impl Payload {
    fn field(&mut self, name: &str) -> &mut Self {
        self.fields.push((self.bytes.len(), name.to_owned()));
        self
    }

    fn u8(&mut self, v: u8) -> &mut Self {
        self.bytes.push(v);
        self
    }

    fn u16(&mut self, v: u16) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_le_bytes());
        self
    }

    fn u32(&mut self, v: u32) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_le_bytes());
        self
    }

    fn u64(&mut self, v: u64) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_le_bytes());
        self
    }

    fn i32(&mut self, v: i32) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_le_bytes());
        self
    }

    fn boolean(&mut self, v: bool) -> &mut Self {
        self.u8(u8::from(v))
    }

    fn str(&mut self, s: &[u8]) -> &mut Self {
        self.u32(u32::try_from(s.len()).expect("a str under 4 GiB"));
        self.bytes.extend_from_slice(s);
        self
    }

    /// Where these bytes first differ from `want`, and the field there; `None` when they agree.
    fn first_difference(&self, want: &[u8]) -> Option<String> {
        if self.bytes == want {
            return None;
        }
        let at = self
            .bytes
            .iter()
            .zip(want)
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| Ord::min(self.bytes.len(), want.len()));
        let field = self
            .fields
            .iter()
            .rev()
            .find(|(start, _)| *start <= at)
            .map_or("the start", |(_, name)| name.as_str());
        Some(format!(
            "byte {at} differs ({} bytes here, {} recorded), in {field}",
            self.bytes.len(),
            want.len()
        ))
    }
}

/// FNV-1a, 64 bits.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for &c in bytes {
        h ^= u64::from(c);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// `probe.hpp`'s SplitMix.
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

fn check(m: &mut Mismatches, rec: &Record, label: &str, p: &Payload) {
    let diff = p.first_difference(&rec.payload);
    m.check(diff.is_none(), || {
        format!("{} {label}: {}", rec.kind(), diff.unwrap_or_default())
    });
}

fn replay<F>(file: &ProbeFile, kind: &str, mut f: F) -> usize
where
    F: FnMut(&Record, &mut Mismatches),
{
    let mut m = Mismatches::default();
    let mut n = 0;
    for rec in file.of_kind(kind) {
        f(rec, &mut m);
        n += 1;
    }
    m.assert_none(&format!("{kind} of {}", file.path.display()));
    n
}

/// A slice of `bytes` from `at`, `n` long, both as the probe's 64-bit counts.
fn sub(bytes: &[u8], at: u64, n: u64) -> &[u8] {
    let at = usize::try_from(at).expect("an offset in memory");
    let n = usize::try_from(n).expect("a length in memory");
    &bytes[at..at + n]
}

fn size(n: u64) -> usize {
    usize::try_from(n).expect("a size in memory")
}

// ---- ZCRC ------------------------------------------------------------------------------------

#[test]
fn zcrc_records_match() {
    let file = zip_file();
    let n = replay(&file, "ZCRC", |rec, m| {
        let mut r = rec.reader();
        let crc = r.u32().unwrap();
        let s = r.bytes().unwrap();
        let mut p = Payload::default();
        p.field("crc").u32(crc).field("bytes").str(s);
        p.field("mz_crc32").u32(mz_crc32(crc, s));
        check(m, rec, &format!("of {} bytes", s.len()), &p);
    });
    assert!(n > 0, "{}: no ZCRC record", file.path.display());
}

// ---- ZINF ------------------------------------------------------------------------------------

/// How `record_inf_one` drives the decompressor.
struct Schedule {
    ring: bool,
    cap: u64,
    flags: u32,
    more_always: bool,
    seed: u64,
    max_in: u32,
    max_out: u32,
}

/// What a schedule's calls gave.
struct InfRun {
    calls: u64,
    log: Vec<u8>,
    first: Vec<u8>,
    status: Status,
    in_pos: u64,
    out_pos: u64,
    adler32: u32,
    produced: Vec<u8>,
}

fn run_inflate(stream: &[u8], s: &Schedule) -> InfRun {
    let mut d = Decompressor::new();
    let mut g = SplitMix(s.seed);
    let mut out = vec![0u8; size(s.cap)];
    let mut run = InfRun {
        calls: 0,
        log: Vec::new(),
        first: Vec::new(),
        status: Status::Failed,
        in_pos: 0,
        out_pos: 0,
        adler32: 0,
        produced: Vec::new(),
    };
    let total = stream.len() as u64;
    loop {
        let in_avail = if s.max_in == 0 {
            total - run.in_pos
        } else {
            Ord::min(1 + g.below(u64::from(s.max_in)), total - run.in_pos)
        };
        let (next, out_size) = if s.ring {
            let next = run.out_pos % s.cap;
            (next, s.cap - next)
        } else {
            let room = s.cap - run.out_pos;
            let out_size = if s.max_out == 0 {
                room
            } else {
                Ord::min(1 + g.below(u64::from(s.max_out)), room)
            };
            (run.out_pos, out_size)
        };
        let wrap = if s.ring {
            0
        } else {
            TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF
        };
        let more = if run.in_pos + in_avail < total || s.more_always {
            TINFL_FLAG_HAS_MORE_INPUT
        } else {
            0
        };
        let input = sub(stream, run.in_pos, in_avail);
        let pr = d.decompress(
            input,
            &mut out,
            size(next),
            size(out_size),
            s.flags | wrap | more,
        );
        run.status = pr.status;
        let mut entry = Vec::with_capacity(20);
        entry.extend_from_slice(&pr.status.code().to_le_bytes());
        entry.extend_from_slice(&(pr.in_consumed as u64).to_le_bytes());
        entry.extend_from_slice(&(pr.out_written as u64).to_le_bytes());
        run.log.extend_from_slice(&entry);
        if run.calls < 16 {
            run.first.extend_from_slice(&entry);
        }
        run.produced
            .extend_from_slice(&out[size(next)..size(next) + pr.out_written]);
        run.in_pos += pr.in_consumed as u64;
        run.out_pos += pr.out_written as u64;
        run.calls += 1;
        if pr.status == Status::NeedsMoreInput && run.in_pos == total {
            break;
        }
        if pr.status != Status::NeedsMoreInput && pr.status != Status::HasMoreOutput {
            break;
        }
        if !s.ring
            && run.out_pos == s.cap
            && pr.status == Status::HasMoreOutput
            && pr.in_consumed == 0
            && pr.out_written == 0
        {
            break;
        }
        if run.calls >= 400_000 {
            break;
        }
    }
    run.adler32 = d.adler32();
    run
}

#[test]
fn zinf_records_match() {
    let file = zip_file();
    let n = replay(&file, "ZINF", |rec, m| {
        let mut r = rec.reader();
        let s = Schedule {
            ring: r.u8().unwrap() != 0,
            cap: r.u64().unwrap(),
            flags: r.u32().unwrap(),
            more_always: r.u8().unwrap() != 0,
            seed: r.u64().unwrap(),
            max_in: r.u32().unwrap(),
            max_out: r.u32().unwrap(),
        };
        let stream = r.bytes().unwrap();
        let mut p = Payload::default();
        p.field("schedule")
            .u8(u8::from(s.ring))
            .u64(s.cap)
            .u32(s.flags)
            .u8(u8::from(s.more_always))
            .u64(s.seed)
            .u32(s.max_in)
            .u32(s.max_out)
            .field("stream")
            .str(stream);
        let run = run_inflate(stream, &s);
        p.field("calls").u64(run.calls);
        p.field("first calls");
        p.bytes.extend_from_slice(&run.first);
        p.field("log")
            .u64(fnv1a(&run.log))
            .field("status")
            .i32(run.status.code())
            .field("in_pos")
            .u64(run.in_pos)
            .field("out_pos")
            .u64(run.out_pos)
            .field("adler32")
            .u32(run.adler32)
            .field("produced")
            .u64(fnv1a(&run.produced))
            .boolean(run.produced.len() <= 256);
        if run.produced.len() <= 256 {
            p.str(&run.produced);
        }
        let label = format!(
            "of a {}-byte stream, seed {}, ring {}, cap {}",
            stream.len(),
            s.seed,
            s.ring,
            s.cap
        );
        check(m, rec, &label, &p);
    });
    assert!(n > 0, "{}: no ZINF record", file.path.display());
}

// ---- ZARC ------------------------------------------------------------------------------------

struct Query {
    name: Vec<u8>,
    flags: u32,
    comment: Option<Vec<u8>>,
}

fn put_stat(p: &mut Payload, s: &FileStat) {
    p.field("stat")
        .u32(s.m_file_index)
        .u64(s.m_central_dir_ofs)
        .u16(s.m_version_made_by)
        .u16(s.m_version_needed)
        .u16(s.m_bit_flag)
        .u16(s.m_method)
        .u32(s.m_crc32);
    p.u64(s.m_comp_size)
        .u64(s.m_uncomp_size)
        .u16(s.m_internal_attr)
        .u32(s.m_external_attr)
        .u64(s.m_local_header_ofs)
        .u32(s.m_comment_size);
    p.boolean(s.m_is_directory)
        .boolean(s.m_is_encrypted)
        .boolean(s.m_is_supported)
        .i32(s.m_is_utf8);
    p.field("stat names").str(&s.m_filename).str(&s.m_comment);
}

/// A folder of the test's own under the temporary folder, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("sketch-orca-zip-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temporary folder");
        TempDir(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A ZARC record's inputs.
struct ArcCase {
    label: Vec<u8>,
    bytes: Vec<u8>,
    how: u8,
    init_flags: u32,
    extra_buf_size: u32,
    fail_at: u32,
    queries: Vec<Query>,
}

fn read_arc_case(r: &mut Reader<'_>) -> ArcCase {
    let label = r.bytes().unwrap().to_vec();
    let bytes = r.bytes().unwrap().to_vec();
    let how = r.u8().unwrap();
    let init_flags = r.u32().unwrap();
    let extra_buf_size = r.u32().unwrap();
    let fail_at = r.u32().unwrap();
    let nq = r.count().unwrap();
    let mut queries = Vec::with_capacity(nq);
    for _ in 0..nq {
        let name = r.bytes().unwrap().to_vec();
        let flags = r.u32().unwrap();
        let comment = if r.bool().unwrap() {
            Some(r.bytes().unwrap().to_vec())
        } else {
            None
        };
        queries.push(Query {
            name,
            flags,
            comment,
        });
    }
    ArcCase {
        label,
        bytes,
        how,
        init_flags,
        extra_buf_size,
        fail_at,
        queries,
    }
}

fn open_case(arc: &mut MzArchive, c: &ArcCase, dir: &Path, archive_path: &Path) -> bool {
    let z = &mut arc.arch;
    match c.how {
        0 => open_zip_reader(z, archive_path),
        1 => z.reader_init_mem(c.bytes.clone(), c.init_flags),
        2 => {
            let src = c.bytes.clone();
            z.set_read_func(Box::new(move |ofs: u64, buf: &mut [u8]| {
                if ofs >= src.len() as u64 {
                    return 0;
                }
                let ofs = size(ofs);
                let k = Ord::min(src.len() - ofs, buf.len());
                buf[..k].copy_from_slice(&src[ofs..ofs + k]);
                k
            }));
            z.reader_init(c.bytes.len() as u64, c.init_flags)
        }
        3 => open_zip_reader(z, &dir.join("missing.zip")),
        _ => open_zip_reader(z, dir),
    }
}

/// An entry's data extracted four ways, as `record_arc_one` does.
fn put_extractions(
    p: &mut Payload,
    arc: &mut MzArchive,
    c: &ArcCase,
    i: u32,
    st: &FileStat,
    out_path: &Path,
) {
    let z = &mut arc.arch;
    {
        let mut buf = vec![0u8; size(st.m_uncomp_size)];
        let ok = z.reader_extract_to_mem(i, &mut buf, 0);
        p.field(&format!("entry {i} extract_to_mem"))
            .boolean(ok)
            .i32(z.m_last_error.code())
            .u64(fnv1a(&buf));
        p.boolean(buf.len() <= 300);
        if buf.len() <= 300 {
            p.str(&buf);
        }
    }
    if st.m_uncomp_size > 0 {
        let mut buf = vec![0u8; size(st.m_uncomp_size - 1)];
        let ok = z.reader_extract_to_mem(i, &mut buf, 0);
        p.field(&format!("entry {i} extract_to_mem short"))
            .boolean(ok)
            .i32(z.m_last_error.code());
    }
    {
        let mut buf = vec![0u8; size(st.m_comp_size)];
        let ok = z.reader_extract_to_mem(i, &mut buf, MZ_ZIP_FLAG_COMPRESSED_DATA);
        p.field(&format!("entry {i} extract_to_mem compressed"))
            .boolean(ok)
            .i32(z.m_last_error.code())
            .u64(fnv1a(&buf));
    }
    {
        let (mut calls, mut log, mut first, mut data) = (0u64, Vec::new(), Vec::new(), Vec::new());
        let mut cb = |ofs: u64, b: &[u8]| -> Result<usize, std::convert::Infallible> {
            let mut e = Vec::with_capacity(16);
            e.extend_from_slice(&ofs.to_le_bytes());
            e.extend_from_slice(&(b.len() as u64).to_le_bytes());
            log.extend_from_slice(&e);
            if calls < 6 {
                first.extend_from_slice(&e);
            }
            data.extend_from_slice(b);
            let fail = calls == u64::from(c.fail_at);
            calls += 1;
            Ok(if fail {
                b.len().wrapping_sub(1)
            } else {
                b.len()
            })
        };
        let Ok(ok) = z.reader_extract_to_callback(i, &mut cb, 0);
        p.field(&format!("entry {i} extract_to_callback"))
            .boolean(ok)
            .i32(z.m_last_error.code())
            .u64(calls);
        p.bytes.extend_from_slice(&first);
        p.u64(fnv1a(&log)).u64(data.len() as u64).u64(fnv1a(&data));
    }
    {
        let _ = std::fs::remove_file(out_path);
        let ok = z.reader_extract_to_file(i, out_path, 0);
        p.field(&format!("entry {i} extract_to_file"))
            .boolean(ok)
            .i32(z.m_last_error.code());
        let contents = std::fs::read(out_path);
        let exists = contents.is_ok();
        let contents = contents.unwrap_or_default();
        p.boolean(exists)
            .u64(contents.len() as u64)
            .u64(fnv1a(&contents));
        let _ = std::fs::remove_file(out_path);
    }
}

fn replay_arc(rec: &Record, m: &mut Mismatches, dir: &Path) {
    let mut r = rec.reader();
    let c = read_arc_case(&mut r);
    let mut p = Payload::default();
    p.field("inputs")
        .str(&c.label)
        .str(&c.bytes)
        .u8(c.how)
        .u32(c.init_flags)
        .u32(c.extra_buf_size)
        .u32(c.fail_at);
    p.u64(c.queries.len() as u64);
    for q in &c.queries {
        p.str(&q.name).u32(q.flags).boolean(q.comment.is_some());
        if let Some(comment) = &q.comment {
            p.str(comment);
        }
    }

    let archive_path = dir.join("a.zip");
    let out_path = dir.join("x.bin");
    if c.how == 0 {
        std::fs::write(&archive_path, &c.bytes).expect("the archive written");
    }
    let mut arc = MzArchive::default();
    let ok = open_case(&mut arc, &c, dir, &archive_path);
    let err = arc.arch.m_last_error.code();
    p.field("open")
        .boolean(ok)
        .i32(err)
        .str(arc.get_errorstr().as_bytes())
        .str(mz_zip_get_error_string(err).as_bytes());
    p.u32(arc.arch.reader_get_num_files())
        .u64(arc.arch.m_archive_size)
        .u64(arc.arch.m_central_directory_file_ofs)
        .boolean(arc.arch.is_zip64());

    let n = if ok {
        Ord::min(arc.arch.reader_get_num_files(), 40)
    } else {
        0
    };
    for i in 0..=n {
        p.field(&format!("entry {i}"));
        let mut st = FileStat::default();
        let sok = arc.arch.reader_file_stat(i, &mut st);
        p.boolean(sok).i32(arc.arch.m_last_error.code());
        if sok {
            put_stat(&mut p, &st);
        }
        let mut extra = vec![0u8; c.extra_buf_size as usize];
        let ret = arc.arch.reader_get_extra(i, &mut extra);
        let k = Ord::min(ret as usize, c.extra_buf_size as usize);
        p.field(&format!("entry {i} extra"))
            .u32(ret)
            .str(&extra[..k])
            .i32(arc.arch.m_last_error.code());
        let sized = sok && st.m_uncomp_size <= MAX_EXTRACT && st.m_comp_size <= MAX_EXTRACT;
        p.boolean(sized);
        if sized {
            put_extractions(&mut p, &mut arc, &c, i, &st, &out_path);
        }
    }
    let z = &mut arc.arch;
    for (qi, q) in c.queries.iter().enumerate() {
        let idx = z.reader_locate_file(&q.name, q.comment.as_deref(), q.flags);
        p.field(&format!("query {qi}"))
            .i32(idx)
            .i32(z.m_last_error.code());
        let mut want = 16u64;
        if let Ok(index) = u32::try_from(idx) {
            let mut st = FileStat::default();
            if z.reader_file_stat(index, &mut st) && st.m_uncomp_size <= MAX_EXTRACT {
                want = st.m_uncomp_size;
            }
        }
        let mut buf = vec![0u8; size(want)];
        let ok = z.reader_extract_file_to_mem(&q.name, &mut buf, q.flags);
        p.u64(want)
            .boolean(ok)
            .i32(z.m_last_error.code())
            .u64(fnv1a(&buf));
    }
    for k in 0..2 {
        let ok = if c.how == 1 || c.how == 2 {
            z.reader_end()
        } else {
            close_zip_reader(z)
        };
        p.field(&format!("close {k}"))
            .boolean(ok)
            .i32(z.m_last_error.code());
    }
    p.u32(z.reader_get_num_files());
    let _ = std::fs::remove_file(&archive_path);
    let label = format!("{} (how {})", String::from_utf8_lossy(&c.label), c.how);
    check(m, rec, &label, &p);
}

#[test]
fn zarc_records_match() {
    let file = zip_file();
    let dir = TempDir::new("arc");
    let n = replay(&file, "ZARC", |rec, m| replay_arc(rec, m, dir.path()));
    assert!(n > 0, "{}: no ZARC record", file.path.display());
}

// ---- ZERR ------------------------------------------------------------------------------------

#[test]
fn zerr_records_match() {
    let file = zip_file();
    let n = replay(&file, "ZERR", |rec, m| {
        let mut p = Payload::default();
        p.field("count").u64(35);
        for code in -1..=33 {
            p.field(&format!("code {code}"))
                .i32(code)
                .str(mz_zip_get_error_string(code).as_bytes())
                .str(get_errorstr(code).as_bytes());
        }
        check(m, rec, "of every code", &p);
    });
    assert!(n > 0, "{}: no ZERR record", file.path.display());
    // The enumeration's codes end at MZ_ZIP_TOTAL_ERRORS.
    assert_eq!(ZipError::TotalErrors.code(), 32);
}

#[test]
fn every_record_kind_is_replayed() {
    let file = zip_file();
    for rec in &file.records {
        assert!(
            matches!(
                rec.kind(),
                "HEAD" | "PRUN" | "ZCRC" | "ZINF" | "ZARC" | "ZERR"
            ),
            "{}: a {} record nothing replays",
            file.path.display(),
            rec.kind()
        );
    }
}
