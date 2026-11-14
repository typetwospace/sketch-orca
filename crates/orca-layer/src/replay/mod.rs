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
// Carries no OrcaSlicer code: it finds and compares the dumps the parity oracle's OracleDump.cpp
// writes (sketch-orca oracle patches 0005 to 0007, parity/SOP1.md) and that the goldens hold
// (parity/README.md). Written on 2026-11-14 (orca-o3-dump-1).

//! The replay of the oracle's step-boundary dumps, for the crates that port process(): find the
//! recorded runs, read a dump into the data model ([`state`]), read any dump's records by kind
//! ([`records`]), and compare a dump written in Rust with the oracle's byte for byte, naming the
//! first record that differs ([`first_difference`]).
//!
//! The goldens hold each run's dumps raw under `cases/<case>/<run>/dumps/` of the folder
//! `SKETCH_ORCA_GOLDENS_DEEP` names ([`goldens_root`], [`golden_runs`]): `front-objects-<n>.sop1`,
//! `front-config-<n>.sop1`, `object<i>-<step>-<n>.sop1` and `print-<step>-<n>.sop1`
//! ([`parse_dump_name`]). A dump the goldens keep only as its hash (`<file>.sha256`, outside
//! domain A) is not a dump file here.
//!
//! [`sop1`] is the container and the payload cursor; [`ReplayError`] says what could not be read
//! and where.

pub mod records;
pub mod sop1;
pub mod state;

use std::path::{Path, PathBuf};

pub use records::{Record, Value, decode_record, read_records};
pub use sop1::{RawRecord, Reader, ReplayError, Result, read_container};
pub use state::{
    BrimEar, FrontConfig, FrontObjects, Head, ObjectDump, read_front_config, read_front_objects,
    read_object_dump, write_front_config, write_object_dump,
};

/// The knob that names the goldens' folder.
pub const GOLDENS_DEEP: &str = "SKETCH_ORCA_GOLDENS_DEEP";

/// The goldens' folder `SKETCH_ORCA_GOLDENS_DEEP` names, none when it is not set or empty.
#[must_use]
pub fn goldens_root() -> Option<PathBuf> {
    std::env::var_os(GOLDENS_DEEP)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// One recorded run: its case, its run's folder name and its dumps' folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenRun {
    /// The case's id (corpus.json).
    pub case: String,
    /// The run's folder name (its index in the case).
    pub run: String,
    /// `cases/<case>/<run>/dumps`.
    pub dumps: PathBuf,
}

impl GoldenRun {
    /// The run's dump files (`.sop1`), sorted by name.
    ///
    /// # Errors
    ///
    /// The folder cannot be listed.
    pub fn files(&self) -> Result<Vec<String>> {
        Ok(list_names(&self.dumps)?
            .into_iter()
            .filter(|n| n.ends_with(".sop1"))
            .collect())
    }

    /// Whether the run has that dump.
    #[must_use]
    pub fn has(&self, file: &str) -> bool {
        self.dumps.join(file).is_file()
    }

    /// A dump's bytes.
    ///
    /// # Errors
    ///
    /// The file cannot be read.
    pub fn read(&self, file: &str) -> Result<Vec<u8>> {
        let path = self.dumps.join(file);
        std::fs::read(&path).map_err(|e| ReplayError::new(format!("{}: {e}", path.display())))
    }
}

/// A folder's entry names, sorted.
fn list_names(folder: &Path) -> Result<Vec<String>> {
    let entries = std::fs::read_dir(folder)
        .map_err(|e| ReplayError::new(format!("{}: {e}", folder.display())))?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| ReplayError::new(format!("{}: {e}", folder.display())))?;
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    orca_stl::sort(&mut names);
    Ok(names)
}

/// Every run of the goldens under `root` that has a dumps folder, by case, then run.
///
/// # Errors
///
/// `root/cases` or a case's folder cannot be listed.
pub fn golden_runs(root: &Path) -> Result<Vec<GoldenRun>> {
    let cases = root.join("cases");
    let mut out = Vec::new();
    for case in list_names(&cases)? {
        let case_dir = cases.join(&case);
        if !case_dir.is_dir() {
            continue;
        }
        for run in list_names(&case_dir)? {
            let dumps = case_dir.join(&run).join("dumps");
            if dumps.is_dir() {
                out.push(GoldenRun {
                    case: case.clone(),
                    run,
                    dumps,
                });
            }
        }
    }
    Ok(out)
}

/// A dump file's name read: `object<i>-<step>-<n>.sop1`, `print-<step>-<n>.sop1` or
/// `front-<name>-<n>.sop1`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DumpName {
    /// "object", "print" or "front".
    pub source: String,
    /// The object's index, for an object dump.
    pub object: Option<i64>,
    /// The step's or the boundary's name.
    pub name: String,
    /// The boundary's count.
    pub occurrence: u32,
}

/// A dump file's name read, none for a name that is not a dump's.
#[must_use]
pub fn parse_dump_name(file: &str) -> Option<DumpName> {
    let stem = file.strip_suffix(".sop1")?;
    let (rest, occurrence) = stem.rsplit_once('-')?;
    let occurrence = occurrence.parse().ok()?;
    let (source, name) = rest.split_once('-')?;
    if let Some(index) = source.strip_prefix("object") {
        let object = index.parse().ok()?;
        return Some(DumpName {
            source: "object".into(),
            object: Some(object),
            name: name.into(),
            occurrence,
        });
    }
    Some(DumpName {
        source: source.into(),
        object: None,
        name: name.into(),
        occurrence,
    })
}

/// Where two dumps first differ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Difference {
    /// The record's index in the file (HEAD is 0), none when the containers themselves differ.
    pub record: Option<usize>,
    /// The record's kind as the expected dump has it ("" when there is none).
    pub kind: String,
    /// The first differing byte, in the record's payload (or in the file, without a record).
    pub offset: usize,
    /// What differs.
    pub detail: String,
}

impl core::fmt::Display for Difference {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.record {
            Some(i) => write!(
                f,
                "record {i} ({}) differs at payload byte {}: {}",
                self.kind, self.offset, self.detail
            ),
            None => write!(
                f,
                "the files differ at byte {}: {}",
                self.offset, self.detail
            ),
        }
    }
}

/// The first byte at which two slices differ, or the shorter one's length.
fn first_byte(a: &[u8], b: &[u8]) -> usize {
    a.iter()
        .zip(b)
        .position(|(x, y)| x != y)
        .unwrap_or(a.len().min(b.len()))
}

/// Where `actual` first differs from `expected`, record by record; none when they are the same
/// bytes.
#[must_use]
pub fn first_difference(expected: &[u8], actual: &[u8]) -> Option<Difference> {
    if expected == actual {
        return None;
    }
    let whole = |detail: String| Difference {
        record: None,
        kind: String::new(),
        offset: first_byte(expected, actual),
        detail,
    };
    let (e, a) = match (read_container(expected), read_container(actual)) {
        (Ok(e), Ok(a)) => (e, a),
        (Err(err), _) => return Some(whole(format!("the expected dump does not read: {err}"))),
        (_, Err(err)) => return Some(whole(format!("the written dump does not read: {err}"))),
    };
    if expected[..8] != actual[..8] {
        return Some(whole("the magic or the version differs".into()));
    }
    for (i, (x, y)) in e.iter().zip(&a).enumerate() {
        if x.kind != y.kind {
            return Some(Difference {
                record: Some(i),
                kind: x.kind_name(),
                offset: 0,
                detail: format!("written as {}", y.kind_name()),
            });
        }
        if x.payload != y.payload {
            return Some(Difference {
                record: Some(i),
                kind: x.kind_name(),
                offset: first_byte(x.payload, y.payload),
                detail: format!(
                    "{} bytes expected, {} written",
                    x.payload.len(),
                    y.payload.len()
                ),
            });
        }
    }
    let i = e.len().min(a.len());
    Some(Difference {
        record: Some(i),
        kind: e.get(i).map(RawRecord::kind_name).unwrap_or_default(),
        offset: 0,
        detail: format!("{} records expected, {} written", e.len(), a.len()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dump::{Dump, Payload, RunState};

    #[test]
    fn dump_names_read_as_their_source_name_and_count() {
        assert_eq!(
            parse_dump_name("object12-posSlice-3.sop1"),
            Some(DumpName {
                source: "object".into(),
                object: Some(12),
                name: "posSlice".into(),
                occurrence: 3,
            })
        );
        assert_eq!(
            parse_dump_name("front-config-1.sop1").map(|d| (d.source, d.name)),
            Some(("front".into(), "config".into()))
        );
        assert_eq!(
            parse_dump_name("print-psConflictCheck-2.sop1").map(|d| d.occurrence),
            Some(2)
        );
        assert_eq!(parse_dump_name("print-psWipeTower-1.sop1.sha256"), None);
        assert_eq!(parse_dump_name("goldens.json"), None);
    }

    #[test]
    fn the_first_difference_names_its_record() {
        let make = |v: u8| {
            let mut dump = Dump::with_run("front", "config", -1, 1, &RunState::default());
            let mut p = Payload::new();
            p.u8(0);
            p.u8(v);
            dump.record(b"FPRT", &p);
            dump.finish().1
        };
        assert_eq!(first_difference(&make(1), &make(1)), None);
        let d = first_difference(&make(1), &make(2)).unwrap();
        assert_eq!(d.record, Some(2));
        assert_eq!(d.kind, "FPRT");
        assert_eq!(d.offset, 1);
        assert!(d.to_string().starts_with("record 2 (FPRT)"));
        let d = first_difference(&make(1), b"nope").unwrap();
        assert_eq!(d.record, None);
    }
}
