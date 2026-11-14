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
// Carries no OrcaSlicer code: it reads the SOP1 container the parity oracle's OracleDump.cpp
// (lines 68-108 and 415-500, sketch-orca oracle patch 0005) writes, as parity/SOP1.md specifies
// it. Written on 2026-11-14 (orca-o3-dump-1).

//! The SOP1 container and the cursor that reads a payload: [`read_container`] splits a dump into
//! its records (the magic "SOP1", version 1, each record a four-letter kind, a u64 length and the
//! payload, END last with the count of the records before it), and [`Reader`] reads the values a
//! payload holds in the order [`crate::dump::Payload`] writes them. Every failure is a
//! [`ReplayError`] that says what was wrong and where.

use crate::dump::SOP1_VERSION;

/// Why a dump could not be read or replayed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayError(pub String);

impl ReplayError {
    /// An error of this text.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// The error with what it happened in put in front.
    #[must_use]
    pub fn context(self, what: &str) -> Self {
        Self(format!("{what}: {}", self.0))
    }
}

impl core::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ReplayError {}

/// A result of the replay.
pub type Result<T> = core::result::Result<T, ReplayError>;

/// One record of a container: its kind and its payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawRecord<'a> {
    /// The four-letter kind ("RUN " and "END " end in a space).
    pub kind: [u8; 4],
    /// The payload.
    pub payload: &'a [u8],
    /// Where the record starts in the file.
    pub offset: usize,
}

impl RawRecord<'_> {
    /// The kind as text.
    #[must_use]
    pub fn kind_name(&self) -> String {
        kind_name(&self.kind)
    }
}

/// A kind as text, its bytes shown as they are.
#[must_use]
pub fn kind_name(kind: &[u8; 4]) -> String {
    String::from_utf8_lossy(kind).into_owned()
}

/// The records of a dump, END included, after checking the magic, the version, each record's
/// length and END's count.
///
/// # Errors
///
/// A file that is not a SOP1 container of version 1, a record that runs past the file, no END,
/// END before the last record or END's count not the number of records before it.
pub fn read_container(bytes: &[u8]) -> Result<Vec<RawRecord<'_>>> {
    if bytes.len() < 8 || &bytes[..4] != b"SOP1" {
        return Err(ReplayError::new("not a SOP1 container (no \"SOP1\" magic)"));
    }
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if version != SOP1_VERSION {
        return Err(ReplayError::new(format!(
            "SOP1 version {version}, not {SOP1_VERSION}"
        )));
    }
    let mut records = Vec::new();
    let mut at = 8;
    while at < bytes.len() {
        if bytes.len() - at < 12 {
            return Err(ReplayError::new(format!(
                "a record header at byte {at} runs past the end of the file"
            )));
        }
        let kind = [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]];
        let mut len = [0u8; 8];
        len.copy_from_slice(&bytes[at + 4..at + 12]);
        let len = u64::from_le_bytes(len);
        let start = at + 12;
        let end = usize::try_from(len)
            .ok()
            .and_then(|l| start.checked_add(l))
            .filter(|&e| e <= bytes.len())
            .ok_or_else(|| {
                ReplayError::new(format!(
                    "record {} ({}) at byte {at} runs past the end of the file",
                    records.len(),
                    kind_name(&kind)
                ))
            })?;
        records.push(RawRecord {
            kind,
            payload: &bytes[start..end],
            offset: at,
        });
        at = end;
    }
    let Some(last) = records.last() else {
        return Err(ReplayError::new("a SOP1 container without records"));
    };
    if &last.kind != b"END " {
        return Err(ReplayError::new("the last record is not END"));
    }
    if let Some(i) = records[..records.len() - 1]
        .iter()
        .position(|r| &r.kind == b"END ")
    {
        return Err(ReplayError::new(format!(
            "END at record {i}, before the last"
        )));
    }
    let mut r = Reader::new(last.payload);
    let count = r.u64()?;
    r.finish()?;
    if count != (records.len() - 1) as u64 {
        return Err(ReplayError::new(format!(
            "END counts {count} records, the file has {}",
            records.len() - 1
        )));
    }
    Ok(records)
}

/// A cursor over a payload, reading what [`crate::dump::Payload`] writes.
#[derive(Clone, Debug)]
pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    /// A cursor at the payload's start.
    #[must_use]
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    /// Where the cursor is.
    #[must_use]
    pub fn position(&self) -> usize {
        self.at
    }

    /// Whether it has read the whole payload.
    #[must_use]
    pub fn at_end(&self) -> bool {
        self.at == self.bytes.len()
    }

    /// Ok when the payload is read to its end.
    ///
    /// # Errors
    ///
    /// Bytes left after the last field.
    pub fn finish(&self) -> Result<()> {
        if self.at_end() {
            Ok(())
        } else {
            Err(ReplayError::new(format!(
                "{} bytes left after the last field (at byte {})",
                self.bytes.len() - self.at,
                self.at
            )))
        }
    }

    /// The next `n` bytes.
    ///
    /// # Errors
    ///
    /// Fewer than `n` bytes left.
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.bytes.len() - self.at < n {
            return Err(ReplayError::new(format!(
                "the payload ends at byte {} where {n} more bytes were wanted (at byte {})",
                self.bytes.len(),
                self.at
            )));
        }
        let out = &self.bytes[self.at..self.at + n];
        self.at += n;
        Ok(out)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    /// A u8.
    ///
    /// # Errors
    ///
    /// The payload ends.
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.array::<1>()?[0])
    }

    /// A u16.
    ///
    /// # Errors
    ///
    /// The payload ends.
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    /// A u32.
    ///
    /// # Errors
    ///
    /// The payload ends.
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    /// A u64.
    ///
    /// # Errors
    ///
    /// The payload ends.
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    /// An i32.
    ///
    /// # Errors
    ///
    /// The payload ends.
    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    /// An i64.
    ///
    /// # Errors
    ///
    /// The payload ends.
    pub fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.array()?))
    }

    /// A bool: 1 or 0, the only bytes the writer makes.
    ///
    /// # Errors
    ///
    /// The payload ends, or another byte.
    pub fn boolean(&mut self) -> Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            b => Err(ReplayError::new(format!(
                "a bool of byte {b} (at byte {})",
                self.at - 1
            ))),
        }
    }

    /// An f32, from its bits.
    ///
    /// # Errors
    ///
    /// The payload ends.
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }

    /// An f64, from its bits.
    ///
    /// # Errors
    ///
    /// The payload ends.
    pub fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_bits(self.u64()?))
    }

    /// A text's bytes: its u32 length, then the bytes.
    ///
    /// # Errors
    ///
    /// The payload ends.
    pub fn str_bytes(&mut self) -> Result<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }

    /// A text as UTF-8.
    ///
    /// # Errors
    ///
    /// The payload ends, or the text is not UTF-8 (a Rust `String` cannot hold it).
    pub fn str(&mut self) -> Result<String> {
        let at = self.at;
        let b = self.str_bytes()?;
        String::from_utf8(b.to_vec())
            .map_err(|_| ReplayError::new(format!("a text that is not UTF-8 (at byte {at})")))
    }

    /// A count: a u64 that must fit the bytes left at `min_size` bytes an element (at least
    /// one), so that a broken count fails here and not in an allocation.
    ///
    /// # Errors
    ///
    /// The payload ends, or the count cannot fit what is left.
    pub fn count(&mut self, min_size: usize) -> Result<usize> {
        let at = self.at;
        let n = self.u64()?;
        let left = (self.bytes.len() - self.at) as u64;
        if n.saturating_mul(orca_stl::minmax::max(min_size, 1) as u64) > left {
            return Err(ReplayError::new(format!(
                "a count of {n} where {left} bytes are left (at byte {at})"
            )));
        }
        usize::try_from(n).map_err(|_| ReplayError::new(format!("a count of {n} (at byte {at})")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dump::{Dump, Payload, RunState};

    #[test]
    fn a_container_reads_back_into_its_records() {
        let mut dump = Dump::with_run("front", "config", -1, 1, &RunState::default());
        let mut p = Payload::new();
        p.str("x");
        p.f64(f64::NAN);
        dump.record(b"FPRT", &p);
        let (_, bytes) = dump.finish();
        let records = read_container(&bytes).unwrap();
        let kinds: Vec<String> = records.iter().map(RawRecord::kind_name).collect();
        assert_eq!(kinds, ["HEAD", "RUN ", "FPRT", "END "]);
        let mut r = Reader::new(records[2].payload);
        assert_eq!(r.str().unwrap(), "x");
        assert_eq!(r.f64().unwrap().to_bits(), f64::NAN.to_bits());
        r.finish().unwrap();
    }

    #[test]
    fn a_broken_container_says_where() {
        assert!(read_container(b"SOP2\x01\x00\x00\x00").is_err());
        let mut bytes = b"SOP1\x01\x00\x00\x00END ".to_vec();
        bytes.extend_from_slice(&8u64.to_le_bytes());
        bytes.extend_from_slice(&1u64.to_le_bytes());
        let e = read_container(&bytes).unwrap_err();
        assert!(e.0.contains("END counts 1"), "{e}");
        let mut r = Reader::new(&[2]);
        assert!(r.boolean().is_err());
        let mut r = Reader::new(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f]);
        assert!(r.count(1).is_err());
    }
}
