// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Reads SOP1 files (`parity/SOP1.md`): the oracle's dumps and the probe program's records.
//!
//! A file is the magic `SOP1`, a `u32` version 1, then records (a four-letter kind, a `u64`
//! length, the payload), the last of which is `END ` with a `u64` count of the records before it.
//! [`parse`] refuses anything else, as the harness's reader does (`scripts/lib/parity/neutral.ts`).
//! A [`Reader`] walks a payload's values: little-endian integers, `f32` and `f64` as their bit
//! patterns, `str` as a `u32` byte count and the bytes, `vec<T>` as a `u64` count and the values.
//!
//! The probe files (`parity/probes/*.sop1`, "Probe files" in `SOP1.md`) start with HEAD and PRUN.
//! [`load_probe`] reads one and checks both: the committed subset in the repository's
//! `parity/probes/` by default, or the full set from the folder `SKETCH_ORCA_PROBES_DEEP` names
//! ([`probe_source`]). The foundation crates' `tests/probes.rs` replay every record with it, and
//! [`Mismatches`] gathers what differs so a failing test lists the first cases together.

use std::fmt;
use std::path::{Path, PathBuf};

/// The magic every SOP1 file starts with.
pub const MAGIC: &[u8; 4] = b"SOP1";

/// The only version there is.
pub const VERSION: u32 = 1;

/// The environment variable naming the folder of the full probe set.
pub const DEEP_ENV: &str = "SKETCH_ORCA_PROBES_DEEP";

/// Why a file or a payload could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn err<T>(msg: impl Into<String>) -> Result<T, Error> {
    Err(Error(msg.into()))
}

/// One record: its four-letter kind (`RUN ` and `END ` end in a space) and its payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub kind: [u8; 4],
    pub payload: Vec<u8>,
}

impl Record {
    /// The kind as text, with its trailing space if it has one.
    #[must_use]
    pub fn kind(&self) -> &str {
        std::str::from_utf8(&self.kind).unwrap_or("????")
    }

    /// A reader over the payload.
    #[must_use]
    pub fn reader(&self) -> Reader<'_> {
        Reader::new(&self.payload)
    }
}

/// A whole SOP1 file: every record before END.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sop1 {
    pub version: u32,
    pub records: Vec<Record>,
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&bytes[at..at + 8]);
    u64::from_le_bytes(b)
}

/// Reads an SOP1 container: the magic, the version, the records and the END record that counts them.
///
/// # Errors
/// Anything else: another magic or version, a record longer than the file, no END, an END whose
/// count is not the number of records, or bytes after END.
pub fn parse(bytes: &[u8]) -> Result<Sop1, Error> {
    if bytes.len() < 8 || &bytes[..4] != MAGIC {
        return err("not an SOP1 file (no SOP1 magic)");
    }
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if version != VERSION {
        return err(format!(
            "SOP1 version {version}, this reader knows {VERSION}"
        ));
    }
    let mut records = Vec::new();
    let mut at = 8usize;
    loop {
        if at + 12 > bytes.len() {
            return err(format!(
                "the file ends inside a record header at byte {at} (no END record)"
            ));
        }
        let kind = [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]];
        let start = at + 12;
        let length = usize::try_from(u64_at(bytes, at + 4)).unwrap_or(usize::MAX);
        if length > bytes.len() - start {
            let k = String::from_utf8_lossy(&kind);
            return err(format!(
                "the {k:?} record at byte {at} is longer than the file"
            ));
        }
        let payload = &bytes[start..start + length];
        at = start + length;
        if &kind == b"END " {
            if payload.len() != 8 {
                return err("the END record is not a u64");
            }
            let count = u64_at(payload, 0);
            if count != records.len() as u64 {
                let n = records.len();
                return err(format!(
                    "the END record counts {count} records, the file has {n}"
                ));
            }
            if at != bytes.len() {
                return err(format!("{} bytes after the END record", bytes.len() - at));
            }
            return Ok(Sop1 { version, records });
        }
        records.push(Record {
            kind,
            payload: payload.to_vec(),
        });
    }
}

/// [`parse`] of a file's bytes.
///
/// # Errors
/// The file cannot be read, or [`parse`] refuses it.
pub fn read_file(path: &Path) -> Result<Sop1, Error> {
    let bytes = std::fs::read(path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
    parse(&bytes).map_err(|e| Error(format!("{}: {e}", path.display())))
}

/// A cursor over a payload, reading values as `SOP1.md` "Values" writes them.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    buf: &'a [u8],
    at: usize,
}

macro_rules! read_le {
    ($($name:ident -> $t:ty, $n:expr;)+) => {
        $(
            #[doc = concat!("A little-endian `", stringify!($t), "`.")]
            ///
            /// # Errors
            /// The payload ends first.
            pub fn $name(&mut self) -> Result<$t, Error> {
                let b = self.take($n)?;
                let mut a = [0u8; $n];
                a.copy_from_slice(b);
                Ok(<$t>::from_le_bytes(a))
            }
        )+
    };
}

impl<'a> Reader<'a> {
    #[must_use]
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, at: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.buf.len() - self.at < n {
            let (len, at) = (self.buf.len(), self.at);
            return err(format!(
                "the payload ends at byte {len}, {n} more were needed at {at}"
            ));
        }
        let s = &self.buf[self.at..self.at + n];
        self.at += n;
        Ok(s)
    }

    read_le! {
        u8 -> u8, 1;
        u16 -> u16, 2;
        u32 -> u32, 4;
        u64 -> u64, 8;
        i32 -> i32, 4;
        i64 -> i64, 8;
    }

    /// A `bool`: a `u8`, 0 or 1.
    ///
    /// # Errors
    /// The payload ends first, or the byte is neither.
    pub fn bool(&mut self) -> Result<bool, Error> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            v => err(format!("a boolean of {v}")),
        }
    }

    /// An `f32` from its bit pattern.
    ///
    /// # Errors
    /// The payload ends first.
    pub fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_bits(self.u32()?))
    }

    /// An `f64` from its bit pattern.
    ///
    /// # Errors
    /// The payload ends first.
    pub fn f64(&mut self) -> Result<f64, Error> {
        Ok(f64::from_bits(self.u64()?))
    }

    /// A `str`'s bytes, as written (not checked to be UTF-8).
    ///
    /// # Errors
    /// The payload ends first.
    pub fn bytes(&mut self) -> Result<&'a [u8], Error> {
        let n = self.u32()? as usize;
        self.take(n)
    }

    /// A `str`.
    ///
    /// # Errors
    /// The payload ends first, or the bytes are not UTF-8.
    pub fn str(&mut self) -> Result<String, Error> {
        let at = self.at;
        let b = self.bytes()?;
        String::from_utf8(b.to_vec())
            .map_err(|_| Error(format!("a str that is not UTF-8 at byte {at}")))
    }

    /// A `vec` count, as a `usize`.
    ///
    /// # Errors
    /// The payload ends first, or the count is more than the payload's bytes.
    pub fn count(&mut self) -> Result<usize, Error> {
        let n = self.u64()?;
        match usize::try_from(n) {
            Ok(c) if c <= self.buf.len() => Ok(c),
            _ => err(format!(
                "a count of {n} in a payload of {} bytes",
                self.buf.len()
            )),
        }
    }

    /// A `vec<T>`, each element read by `f`.
    ///
    /// # Errors
    /// What the count or `f` gives.
    pub fn vec<T>(
        &mut self,
        mut f: impl FnMut(&mut Self) -> Result<T, Error>,
    ) -> Result<Vec<T>, Error> {
        let n = self.count()?;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(f(self)?);
        }
        Ok(v)
    }

    /// Whether every byte has been read.
    #[must_use]
    pub fn done(&self) -> bool {
        self.at == self.buf.len()
    }

    /// The bytes not read yet.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.buf.len() - self.at
    }

    /// Requires every byte read.
    ///
    /// # Errors
    /// Bytes are left.
    pub fn finish(&self, what: &str) -> Result<(), Error> {
        if self.done() {
            Ok(())
        } else {
            err(format!(
                "the {what} record has {} bytes after its fields",
                self.remaining()
            ))
        }
    }
}

/// A HEAD record (`SOP1.md`): the source file or `probe`, the name, the object's id, its occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub source: String,
    pub name: String,
    pub object: i64,
    pub occurrence: u32,
}

/// Reads a HEAD record's payload.
///
/// # Errors
/// The payload is not a HEAD's fields.
pub fn head(record: &Record) -> Result<Head, Error> {
    let mut r = record.reader();
    let h = Head {
        source: r.str()?,
        name: r.str()?,
        object: r.i64()?,
        occurrence: r.u32()?,
    };
    r.finish("HEAD")?;
    Ok(h)
}

/// A probe file's PRUN record: the compiler and STL the probe was built with, the UCRT's FMA3
/// choice at start, and the set (`committed` or `full`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prun {
    pub msc_full_ver: u32,
    pub msvc_stl_update: u32,
    pub fma3_at_start: i32,
    pub set: String,
}

/// Reads a PRUN record's payload.
///
/// # Errors
/// The payload is not a PRUN's fields.
pub fn prun(record: &Record) -> Result<Prun, Error> {
    let mut r = record.reader();
    let p = Prun {
        msc_full_ver: r.u32()?,
        msvc_stl_update: r.u32()?,
        fma3_at_start: r.i32()?,
        set: r.str()?,
    };
    r.finish("PRUN")?;
    Ok(p)
}

/// Which probe set the tests replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeSet {
    /// The repository's `parity/probes/`.
    Committed,
    /// The folder `SKETCH_ORCA_PROBES_DEEP` names (the work folder's `parity/probes`).
    Full,
}

impl ProbeSet {
    /// The set's name in PRUN.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Committed => "committed",
            Self::Full => "full",
        }
    }
}

/// The repository's committed probe subset: `parity/probes/` beside `crates/`.
#[must_use]
pub fn committed_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("parity")
        .join("probes")
}

/// The folder to replay and its set: `SKETCH_ORCA_PROBES_DEEP` when it is set and not empty,
/// else the committed subset.
#[must_use]
pub fn probe_source() -> (PathBuf, ProbeSet) {
    match std::env::var_os(DEEP_ENV) {
        Some(v) if !v.is_empty() => (PathBuf::from(v), ProbeSet::Full),
        _ => (committed_dir(), ProbeSet::Committed),
    }
}

/// A probe file, its HEAD and PRUN checked and taken off.
#[derive(Debug, Clone)]
pub struct ProbeFile {
    pub path: PathBuf,
    pub set: ProbeSet,
    pub prun: Prun,
    /// Every record after HEAD and PRUN.
    pub records: Vec<Record>,
}

impl ProbeFile {
    /// The records of one kind, in order.
    pub fn of_kind<'s>(&'s self, kind: &'s str) -> impl Iterator<Item = &'s Record> + 's {
        self.records.iter().filter(move |r| r.kind() == kind)
    }
}

/// Reads `<folder>/<name>.sop1` of [`probe_source`]'s folder and checks that it is the probe's
/// file of that name and set: HEAD `probe`, `name`, -1, 1, then PRUN of the set.
///
/// # Errors
/// The file is missing or unreadable, or its HEAD or PRUN is not that.
pub fn load_probe(name: &str) -> Result<ProbeFile, Error> {
    let (dir, set) = probe_source();
    load_probe_from(&dir, set, name)
}

/// [`load_probe`] from a given folder and set.
///
/// # Errors
/// As [`load_probe`].
pub fn load_probe_from(dir: &Path, set: ProbeSet, name: &str) -> Result<ProbeFile, Error> {
    let path = dir.join(format!("{name}.sop1"));
    let shown = path.display().to_string();
    let file = read_file(&path)?;
    let mut it = file.records.into_iter();
    let (Some(h), Some(p)) = (it.next(), it.next()) else {
        return err(format!("{shown}: no HEAD and PRUN"));
    };
    if h.kind() != "HEAD" || p.kind() != "PRUN" {
        return err(format!(
            "{shown}: starts with {} and {}, not HEAD and PRUN",
            h.kind(),
            p.kind()
        ));
    }
    let head = head(&h)?;
    if head.source != "probe" || head.name != name || head.object != -1 || head.occurrence != 1 {
        return err(format!("{shown}: HEAD is {head:?}, not the probe's {name}"));
    }
    let prun = prun(&p)?;
    if prun.set != set.name() {
        return err(format!(
            "{shown}: the {} set, not the {}",
            prun.set,
            set.name()
        ));
    }
    Ok(ProbeFile {
        path,
        set,
        prun,
        records: it.collect(),
    })
}

/// Collects mismatches and reports them together: a probe test fails with the first ones listed.
#[derive(Debug, Default)]
pub struct Mismatches {
    pub checked: u64,
    pub count: u64,
    pub list: Vec<String>,
}

impl Mismatches {
    /// Counts one comparison; records `what` when `same` is false.
    pub fn check(&mut self, same: bool, what: impl FnOnce() -> String) {
        self.checked += 1;
        if !same {
            self.count += 1;
            if self.list.len() < 40 {
                self.list.push(what());
            }
        }
    }

    /// Panics with the first mismatches unless there are none.
    ///
    /// # Panics
    /// When any comparison failed, or none was made.
    pub fn assert_none(&self, what: &str) {
        assert!(self.checked > 0, "{what}: nothing was compared");
        assert!(
            self.count == 0,
            "{what}: {} of {} comparisons differ from the C++ engine's:\n{}",
            self.count,
            self.checked,
            self.list.join("\n")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(records: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let mut b = Vec::from(&MAGIC[..]);
        b.extend_from_slice(&VERSION.to_le_bytes());
        for (k, p) in records {
            b.extend_from_slice(&k[..]);
            b.extend_from_slice(&(p.len() as u64).to_le_bytes());
            b.extend_from_slice(p);
        }
        b.extend_from_slice(b"END ");
        b.extend_from_slice(&8u64.to_le_bytes());
        b.extend_from_slice(&(records.len() as u64).to_le_bytes());
        b
    }

    #[test]
    fn reads_records_and_values() {
        let mut p = Vec::new();
        p.extend_from_slice(&3u32.to_le_bytes());
        p.extend_from_slice(b"abc");
        p.extend_from_slice(&(-2i64).to_le_bytes());
        p.extend_from_slice(&2u64.to_le_bytes());
        p.extend_from_slice(&1.5f64.to_bits().to_le_bytes());
        p.extend_from_slice(&(-0.0f64).to_bits().to_le_bytes());
        p.push(1);
        let f = parse(&file(&[(b"TEST", &p)])).unwrap();
        assert_eq!(f.records.len(), 1);
        assert_eq!(f.records[0].kind(), "TEST");
        let mut r = f.records[0].reader();
        assert_eq!(r.str().unwrap(), "abc");
        assert_eq!(r.i64().unwrap(), -2);
        let v = r.vec(Reader::f64).unwrap();
        assert_eq!(v[0].to_bits(), 1.5f64.to_bits());
        assert_eq!(v[1].to_bits(), (-0.0f64).to_bits());
        assert!(r.bool().unwrap());
        assert!(r.done());
        assert!(r.u8().is_err());
    }

    #[test]
    fn refuses_what_is_not_sop1() {
        assert!(parse(b"SOP2\x01\0\0\0").is_err());
        let mut f = file(&[(b"TEST", b"x")]);
        let n = f.len();
        f[n - 8] = 2;
        assert!(parse(&f).unwrap_err().0.contains("counts 2"));
        let mut g = file(&[]);
        g.push(0);
        assert!(parse(&g).unwrap_err().0.contains("after the END"));
        let h = file(&[(b"TEST", b"xyz")]);
        assert!(parse(&h[..14]).is_err());
    }

    /// The SOP1 files of the probe registry (`scripts/lib/parity/probe.ts`, `PROBE_REGISTRY`): the
    /// foundations', which hold records, and phase O2's (2026-11-07), stubs until their tasks fill them.
    const FOUNDATION_PROBES: [&str; 4] = ["crt", "fmt", "stl", "tbb"];
    const O2_PROBES: [&str; 9] = [
        "geom",
        "qhull",
        "mesh",
        "cstdio",
        "zip",
        "xml",
        "model",
        "arrange",
        "placeholder",
    ];

    #[test]
    fn the_committed_probe_files_carry_their_head_and_prun() {
        for name in FOUNDATION_PROBES.into_iter().chain(O2_PROBES) {
            let f = load_probe_from(&committed_dir(), ProbeSet::Committed, name).unwrap();
            assert!(
                !FOUNDATION_PROBES.contains(&name) || !f.records.is_empty(),
                "{name}"
            );
            assert!(
                (194_200_000..194_300_000).contains(&f.prun.msc_full_ver),
                "{name}: {:?}",
                f.prun
            );
            assert!(load_probe_from(&committed_dir(), ProbeSet::Full, name).is_err());
        }
    }

    #[test]
    fn mismatches_report_together() {
        let mut m = Mismatches::default();
        m.check(true, || unreachable!());
        assert_eq!((m.checked, m.count), (1, 0));
        m.assert_none("one check");
        m.check(false, || "x".into());
        assert!(std::panic::catch_unwind(|| m.assert_none("two checks")).is_err());
    }
}
