// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-xml: the records of `xml.sop1` (`parity/SOP1.md`, "Probe files"), from
//! the committed subset (`parity/probes/`), or from the full set when `SKETCH_ORCA_PROBES_DEEP`
//! names its folder (`<work>/parity/probes`).
//!
//! Each record is rebuilt here byte for byte as `parity/oracle/probe/xml.cpp` writes it, from the
//! inputs it holds and what this crate does with them, and the two are compared whole; a
//! difference is named by the field it falls in. The kinds: XPRS (a document fed to a parser in
//! chunks or through its buffer, with the callbacks, stops, statuses, error, line and column) and
//! XERR (the error texts). Filled by o2-model (2026-11-08).

use orca_probe::{Mismatches, ProbeFile, Reader, Record, load_probe};
use orca_xml::{Ctl, Handler, Parser, Parsing, Status, error_string};

fn xml_file() -> ProbeFile {
    load_probe("xml").unwrap_or_else(|e| panic!("{e}"))
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

    /// Appends `other`'s bytes and fields.
    fn append(&mut self, other: Payload) -> &mut Self {
        let base = self.bytes.len();
        self.fields
            .extend(other.fields.into_iter().map(|(at, name)| (base + at, name)));
        self.bytes.extend_from_slice(&other.bytes);
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

// ---- XPRS ------------------------------------------------------------------------------------

/// `xml.cpp`'s feedings.
const PARSE_CHUNKS: u8 = 0;
const BUFFER_WHOLE: u8 = 1;
const PARSE_THEN_FINAL: u8 = 3;
const BUFFER_CHUNKS: u8 = 4;

/// A record's inputs.
struct Case<'a> {
    label: String,
    mode: u8,
    doc: &'a [u8],
    pattern: Vec<u32>,
    stop_at: i32,
    stop_after: bool,
}

fn read_case<'a>(r: &mut Reader<'a>) -> Case<'a> {
    let label = r.str().expect("label");
    let mode = r.u8().expect("mode");
    let doc = r.bytes().expect("document");
    let n = r.count().expect("pattern count");
    let pattern = (0..n).map(|_| r.u32().expect("pattern")).collect();
    let stop_at = r.i32().expect("stop at");
    let stop_after = r.bool().expect("stop after");
    Case {
        label,
        mode,
        doc,
        pattern,
        stop_at,
        stop_after,
    }
}

/// `chunk_sizes`: the pattern repeated over `n` bytes, the last chunk cut; one chunk of all of
/// them for an empty pattern, none for an empty document.
fn chunk_sizes(n: usize, pattern: &[u32]) -> Vec<usize> {
    if pattern.is_empty() {
        return if n == 0 { Vec::new() } else { vec![n] };
    }
    let mut out = Vec::new();
    let mut at = 0;
    let mut i = 0;
    while at < n {
        let k = Ord::min(pattern[i % pattern.len()] as usize, n - at);
        out.push(k);
        at += k;
        i += 1;
    }
    out
}

/// The handlers' state: the callbacks so far and the stop plan.
struct Rec {
    events: Payload,
    count: u64,
    stop_at: i32,
    stop_after: bool,
    /// The importer's `parse_error()`.
    stopped: bool,
}

impl Rec {
    fn after(&mut self, ctl: &mut Ctl) {
        let index = i64::try_from(self.count).expect("a callback count");
        self.count += 1;
        let at = i64::from(self.stop_at);
        if self.stop_at >= 0 && (index == at || (self.stop_after && index > at)) {
            self.stopped = true;
            let st = ctl.stop_parser(false);
            self.events.i32(st as i32);
        } else {
            self.events.i32(-1);
        }
    }
}

impl Handler for Rec {
    fn start_element(&mut self, ctl: &mut Ctl, name: &[u8], atts: &[&[u8]]) {
        let i = self.count;
        self.events
            .field(&format!("callback {i} (start)"))
            .u8(b'S')
            .str(name)
            .i32(ctl.specified_attribute_count())
            .u64(atts.len() as u64);
        for a in atts {
            self.events.str(a);
        }
        self.after(ctl);
    }

    fn end_element(&mut self, ctl: &mut Ctl, name: &[u8]) {
        let i = self.count;
        self.events
            .field(&format!("callback {i} (end)"))
            .u8(b'E')
            .str(name);
        self.after(ctl);
    }

    fn character_data(&mut self, ctl: &mut Ctl, s: &[u8]) {
        let i = self.count;
        self.events
            .field(&format!("callback {i} (characters)"))
            .u8(b'C')
            .str(s);
        self.after(ctl);
    }
}

fn parsing_number(p: Parsing) -> i32 {
    match p {
        Parsing::Initialized => 0,
        Parsing::Parsing => 1,
        Parsing::Finished => 2,
        Parsing::Suspended => 3,
    }
}

fn int(n: usize) -> i32 {
    i32::try_from(n).expect("a length in an int")
}

/// `run_case`: the record `c` makes, and whether the parse ended in an error.
fn run_case(c: &Case<'_>) -> (Payload, bool) {
    let mut p = Parser::new();
    let mut r = Rec {
        events: Payload::default(),
        count: 0,
        stop_at: c.stop_at,
        stop_after: c.stop_after,
        stopped: false,
    };
    let mut calls = Payload::default();
    let mut ncalls = 0u64;
    let mut buffer_null = false;
    let sizes = chunk_sizes(c.doc.len(), &c.pattern);
    let mut call = |calls: &mut Payload, n: usize, st: Status| {
        calls
            .field(&format!("call {ncalls}"))
            .u32(u32::try_from(n).expect("a chunk under 4 GiB"))
            .i32(st as i32);
        ncalls += 1;
    };
    if c.mode == BUFFER_WHOLE {
        match p.get_buffer(int(c.doc.len())) {
            None => buffer_null = true,
            Some(b) => {
                b.copy_from_slice(c.doc);
                let st = p.parse_buffer(&mut r, int(c.doc.len()), true);
                call(&mut calls, c.doc.len(), st);
            }
        }
    } else if c.mode == BUFFER_CHUNKS {
        let mut at = 0;
        for (i, &n) in sizes.iter().enumerate() {
            match p.get_buffer(int(n)) {
                None => {
                    buffer_null = true;
                    break;
                }
                Some(b) => b.copy_from_slice(&c.doc[at..at + n]),
            }
            at += n;
            let st = p.parse_buffer(&mut r, int(n), i + 1 == sizes.len());
            call(&mut calls, n, st);
            if st != Status::Ok || r.stopped {
                break;
            }
        }
    } else {
        let mut at = 0;
        let mut going = true;
        for (i, &n) in sizes.iter().enumerate() {
            let fin = c.mode == PARSE_CHUNKS && i + 1 == sizes.len();
            let st = p.parse(&mut r, &c.doc[at..at + n], fin);
            at += n;
            call(&mut calls, n, st);
            if st != Status::Ok || r.stopped {
                going = false;
                break;
            }
        }
        if c.mode == PARSE_THEN_FINAL && going {
            let st = p.parse(&mut r, &[], true);
            call(&mut calls, 0, st);
        }
    }

    let code = p.error_code();
    let text = error_string(code.code());
    let line = p.current_line_number();
    let col = p.current_column_number();
    let parsing = p.parsing();
    let spec = p.specified_attribute_count();

    let mut out = Payload::default();
    out.field("label")
        .str(c.label.as_bytes())
        .field("mode")
        .u8(c.mode)
        .field("document")
        .str(c.doc)
        .field("pattern")
        .u64(c.pattern.len() as u64);
    for &x in &c.pattern {
        out.u32(x);
    }
    out.field("stop at")
        .i32(c.stop_at)
        .field("stop after")
        .boolean(c.stop_after)
        .field("buffer null")
        .boolean(buffer_null)
        .field("calls")
        .u64(ncalls)
        .append(calls)
        .field("callbacks")
        .u64(r.count)
        .append(r.events)
        .field("error code")
        .i32(code.code())
        .field("error string")
        .boolean(text.is_some())
        .str(text.unwrap_or("").as_bytes())
        .field("line")
        .u32(line)
        .field("column")
        .u32(col)
        .field("parsing")
        .i32(parsing_number(parsing))
        .field("specified attribute count")
        .i32(spec);
    (out, code.code() != 0)
}

#[test]
fn xprs_records_match() {
    let file = xml_file();
    let mut modes = [0usize; 5];
    let mut stops = 0;
    let mut errors = 0;
    let n = replay(&file, "XPRS", |rec, m| {
        let mut r = rec.reader();
        let c = read_case(&mut r);
        modes[usize::from(c.mode)] += 1;
        stops += usize::from(c.stop_at >= 0);
        let (p, failed) = run_case(&c);
        errors += usize::from(failed);
        let label = format!("\"{}\" (mode {}, pattern {:?})", c.label, c.mode, c.pattern);
        check(m, rec, &label, &p);
    });
    assert!(n > 0, "{}: no XPRS record", file.path.display());
    assert!(
        modes.iter().all(|&k| k > 0),
        "{}: a feeding no record uses: {modes:?}",
        file.path.display()
    );
    assert!(
        stops > 0 && errors > 0,
        "{}: no stop or no error",
        file.path.display()
    );
}

// ---- XERR ------------------------------------------------------------------------------------

#[test]
fn xerr_records_match() {
    let file = xml_file();
    let n = replay(&file, "XERR", |rec, m| {
        let mut p = Payload::default();
        p.field("count").u64(46);
        for code in -1..=44 {
            let s = error_string(code);
            p.field(&format!("code {code}"))
                .i32(code)
                .boolean(s.is_some())
                .str(s.unwrap_or("").as_bytes());
        }
        check(m, rec, "of every code", &p);
    });
    assert!(n > 0, "{}: no XERR record", file.path.display());
}

#[test]
fn every_record_kind_is_replayed() {
    let file = xml_file();
    for rec in &file.records {
        assert!(
            matches!(rec.kind(), "HEAD" | "PRUN" | "XPRS" | "XERR"),
            "{}: a {} record nothing replays",
            file.path.display(),
            rec.kind()
        );
    }
}
