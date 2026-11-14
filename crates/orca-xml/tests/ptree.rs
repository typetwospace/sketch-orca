// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-xml's `property_tree`: the records of `ptree.sop1` (`parity/SOP1.md`,
//! "Probe files"), from the committed subset (`parity/probes/`), or from the full set when
//! `SKETCH_ORCA_PROBES_DEEP` names its folder (`<work>/parity/probes`).
//!
//! Each record is rebuilt here byte for byte as `parity/oracle/probe/ptree.cpp` writes it, from the
//! inputs it holds and what this crate does with them, and the two are compared whole; a
//! difference is named by the field it falls in. The kinds: PTXR (a document through `read_xml`:
//! the `what()` it threw, or the whole tree) and PTGT (a document through `read_xml`, then the
//! reader's calls on the tree, each answer or the `what()` it threw). Filled by orca-ptree-port
//! (2026-11-11).
//!
//! The replay walks a tree over stacks of its own (`put_node`, `locate`), so it writes and searches
//! a tree of any depth; `put_node` is held to that on a million levels and a 256 KiB stack
//! (orca-xml-stack, 2026-11-11).

use orca_probe::{Mismatches, ProbeFile, Reader, Record, load_probe};
use orca_xml::{Ptree, PtreeError, read_xml};

fn ptree_file() -> ProbeFile {
    load_probe("ptree").unwrap_or_else(|e| panic!("{e}"))
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

    fn f32(&mut self, v: f32) -> &mut Self {
        self.u32(v.to_bits())
    }

    fn f64(&mut self, v: f64) -> &mut Self {
        self.u64(v.to_bits())
    }

    fn str(&mut self, s: &[u8]) -> &mut Self {
        self.u32(u32::try_from(s.len()).expect("a str under 4 GiB"));
        self.bytes.extend_from_slice(s);
        self
    }

    fn u64s(&mut self, v: &[u64]) -> &mut Self {
        self.u64(v.len() as u64);
        for &x in v {
            self.u64(x);
        }
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
        format!("{} \"{label}\": {}", rec.kind(), diff.unwrap_or_default())
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

// ---- PTXR ------------------------------------------------------------------------------------

/// `put_node`: a node and everything under it, in document order: key, data, number of children,
/// children. `ptree.cpp`'s `put_node` calls itself for each child; here each node whose children
/// are being written keeps the rest of them on a stack of their own, innermost last, so the bytes
/// come in the same order and no depth of tree overflows the test's stack.
fn put_node(p: &mut Payload, key: &[u8], t: &Ptree) {
    p.str(key).str(t.data()).u64(t.size() as u64);
    let mut stack = vec![t.children()];
    while let Some(children) = stack.last_mut() {
        if let Some((k, c)) = children.next() {
            p.str(k).str(c.data()).u64(c.size() as u64);
            stack.push(c.children());
        } else {
            stack.pop();
        }
    }
}

/// `put_node` on a tree of a million levels, on a thread with a 256 KiB stack: every level's key,
/// data and number of children, outermost first, with no frame per level.
#[test]
fn put_node_writes_a_million_levels_on_a_small_stack() {
    const LEVELS: usize = 1_000_000;
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut doc = b"<a>".repeat(LEVELS - 1);
            doc.extend_from_slice(b"<a>x");
            doc.extend_from_slice(&b"</a>".repeat(LEVELS));
            let tree = read_xml(&doc).expect("the document reads");
            let mut p = Payload::default();
            put_node(&mut p, b"", &tree);
            let mut want = Payload::default();
            want.str(b"").str(b"").u64(1);
            for level in 1..=LEVELS {
                let last = level == LEVELS;
                want.str(b"a")
                    .str(if last { b"x" } else { b"" })
                    .u64(u64::from(!last));
            }
            assert_eq!(p.first_difference(&want.bytes), None);
        })
        .expect("a thread")
        .join()
        .expect("the thread's work");
}

#[test]
fn ptxr_records_match() {
    let file = ptree_file();
    let mut failed = 0;
    let n = replay(&file, "PTXR", |rec, m| {
        let mut r = rec.reader();
        let label = r.str().expect("label");
        let doc = r.bytes().expect("document");
        let mut p = Payload::default();
        p.field("label").str(label.as_bytes());
        p.field("document").str(doc);
        match read_xml(doc) {
            Err(e) => {
                failed += 1;
                p.field("what").u8(1).str(e.what().as_bytes());
            }
            Ok(tree) => {
                p.field("tree").u8(0);
                put_node(&mut p, b"", &tree);
            }
        }
        check(m, rec, &label, &p);
    });
    assert!(n > 0, "{}: no PTXR record", file.path.display());
    assert!(
        failed > 0 && failed < n,
        "{}: PTXR records that all fail or none",
        file.path.display()
    );
}

// ---- PTGT ------------------------------------------------------------------------------------

/// `ptree.cpp`'s calls.
const GET_CHILD: u8 = 0;
const GET_INT: u8 = 1;
const GET_SIZE: u8 = 2;
const GET_FLOAT: u8 = 3;
const GET_DOUBLE: u8 = 4;
const GET_STRING: u8 = 5;
const DEF_INT: u8 = 6;
const DEF_SIZE: u8 = 7;
const DEF_FLOAT: u8 = 8;
const DEF_DOUBLE: u8 = 9;
const DEF_STRING: u8 = 10;
const DATA: u8 = 11;
const CHILDREN: u8 = 12;

/// A query's default, by its call.
enum Default<'a> {
    None,
    Int(i32),
    Size(u64),
    Float(f32),
    Double(f64),
    String(&'a [u8]),
}

/// A query a record holds: the node it is made on, the call, its path and default.
struct Query<'a> {
    at: Vec<u64>,
    call: u8,
    path: &'a [u8],
    default: Default<'a>,
}

fn read_query<'a>(r: &mut Reader<'a>) -> Query<'a> {
    let n = r.count().expect("at count");
    let at = (0..n).map(|_| r.u64().expect("at")).collect();
    let call = r.u8().expect("call");
    let path = r.bytes().expect("path");
    let default = match call {
        DEF_INT => Default::Int(r.i32().expect("int default")),
        DEF_SIZE => Default::Size(r.u64().expect("size default")),
        DEF_FLOAT => Default::Float(r.f32().expect("float default")),
        DEF_DOUBLE => Default::Double(r.f64().expect("double default")),
        DEF_STRING => Default::String(r.bytes().expect("string default")),
        _ => Default::None,
    };
    Query {
        at,
        call,
        path,
        default,
    }
}

/// Steps past the answer a record holds for a query, to reach the next one.
fn skip_answer(r: &mut Reader<'_>, call: u8) {
    if r.u8().expect("threw") == 1 {
        r.bytes().expect("what");
        return;
    }
    match call {
        GET_CHILD => {
            let n = r.count().expect("where count");
            for _ in 0..n {
                r.u64().expect("where");
            }
        }
        GET_INT | DEF_INT => {
            r.i32().expect("int");
        }
        GET_SIZE | DEF_SIZE => {
            r.u64().expect("size");
        }
        GET_FLOAT | DEF_FLOAT => {
            r.f32().expect("float");
        }
        GET_DOUBLE | DEF_DOUBLE => {
            r.f64().expect("double");
        }
        GET_STRING | DEF_STRING | DATA => {
            r.bytes().expect("string");
        }
        CHILDREN => {
            let n = r.count().expect("children");
            for _ in 0..n {
                r.bytes().expect("key");
                r.bytes().expect("data");
                r.u64().expect("its children");
            }
        }
        other => panic!("a call ptree.cpp does not make: {other}"),
    }
}

/// `node_at`: the node reached by child indexes from the root.
fn node_at<'t>(root: &'t Ptree, at: &[u64]) -> Option<&'t Ptree> {
    let mut n = root;
    for &i in at {
        let i = usize::try_from(i).ok()?;
        n = n.children().nth(i)?.1;
    }
    Some(n)
}

/// `locate`: the child indexes from `from` to `target`, the node `get_child` returned, searched
/// depth first in document order. Each node whose children are being searched keeps the rest of
/// them on a stack, innermost last; `out` holds the indexes down to the node being looked at.
fn locate(from: &Ptree, target: &Ptree, out: &mut Vec<u64>) -> bool {
    if std::ptr::eq(from, target) {
        return true;
    }
    let mut stack = vec![from.children().enumerate()];
    while let Some(children) = stack.last_mut() {
        if let Some((i, (_, c))) = children.next() {
            out.push(i as u64);
            if std::ptr::eq(c, target) {
                return true;
            }
            stack.push(c.children().enumerate());
        } else {
            stack.pop();
            if !stack.is_empty() {
                // Done with this node: its index goes.
                out.pop();
            }
        }
    }
    false
}

/// `put_query`: the query, then `u8 0` and the answer or `u8 1` and the `what()` it threw.
fn put_query(p: &mut Payload, root: &Ptree, x: &Query<'_>, i: usize) {
    p.field(&format!("query {i} (call {})", x.call))
        .u64s(&x.at)
        .u8(x.call)
        .str(x.path);
    match x.default {
        Default::None => {}
        Default::Int(d) => {
            p.i32(d);
        }
        Default::Size(d) => {
            p.u64(d);
        }
        Default::Float(d) => {
            p.f32(d);
        }
        Default::Double(d) => {
            p.f64(d);
        }
        Default::String(d) => {
            p.str(d);
        }
    }
    let Some(n) = node_at(root, &x.at) else {
        // The record's node is not in this tree: nothing to ask, and the bytes differ here.
        p.field(&format!("query {i}: its node")).u8(0xFF);
        return;
    };
    let mut a = Payload::default();
    let answer: Result<(), PtreeError> = (|| {
        match x.call {
            GET_CHILD => {
                let c = n.get_child(x.path)?;
                let mut at = Vec::new();
                assert!(
                    locate(n, c, &mut at),
                    "get_child gave a node outside its tree"
                );
                a.u64s(&at);
            }
            GET_INT => {
                a.i32(n.get::<i32>(x.path)?);
            }
            GET_SIZE => {
                a.u64(n.get::<u64>(x.path)?);
            }
            GET_FLOAT => {
                a.f32(n.get::<f32>(x.path)?);
            }
            GET_DOUBLE => {
                a.f64(n.get::<f64>(x.path)?);
            }
            GET_STRING => {
                a.str(&n.get::<Vec<u8>>(x.path)?);
            }
            DEF_INT => {
                let Default::Int(d) = x.default else {
                    unreachable!()
                };
                a.i32(n.get_or::<i32>(x.path, d));
            }
            DEF_SIZE => {
                let Default::Size(d) = x.default else {
                    unreachable!()
                };
                a.u64(n.get_or::<u64>(x.path, d));
            }
            DEF_FLOAT => {
                let Default::Float(d) = x.default else {
                    unreachable!()
                };
                a.f32(n.get_or::<f32>(x.path, d));
            }
            DEF_DOUBLE => {
                let Default::Double(d) = x.default else {
                    unreachable!()
                };
                a.f64(n.get_or::<f64>(x.path, d));
            }
            DEF_STRING => {
                let Default::String(d) = x.default else {
                    unreachable!()
                };
                a.str(&n.get_or::<Vec<u8>>(x.path, d.to_vec()));
            }
            DATA => {
                a.str(n.data());
            }
            CHILDREN => {
                a.u64(n.size() as u64);
                for (k, c) in n.children() {
                    a.str(k).str(c.data()).u64(c.size() as u64);
                }
            }
            other => panic!("a call ptree.cpp does not make: {other}"),
        }
        Ok(())
    })();
    match answer {
        Ok(()) => {
            p.field(&format!("query {i}: its answer")).u8(0).append(a);
        }
        Err(e) => {
            p.field(&format!("query {i}: what it threw"))
                .u8(1)
                .str(e.what());
        }
    }
}

#[test]
fn ptgt_records_match() {
    let file = ptree_file();
    let mut queries = 0usize;
    let mut calls = [0usize; 13];
    let n = replay(&file, "PTGT", |rec, m| {
        let mut r = rec.reader();
        let label = r.str().expect("label");
        let doc = r.bytes().expect("document");
        // The record's queries: its inputs, past each answer.
        let mut qs = Vec::new();
        if r.u8().expect("read failed") == 0 {
            let nq = r.count().expect("query count");
            for _ in 0..nq {
                let q = read_query(&mut r);
                skip_answer(&mut r, q.call);
                qs.push(q);
            }
        } else {
            r.bytes().expect("what");
        }
        r.finish("a PTGT record").expect("the record read whole");

        let mut p = Payload::default();
        p.field("label").str(label.as_bytes());
        p.field("document").str(doc);
        match read_xml(doc) {
            Err(e) => {
                p.field("what").u8(1).str(e.what().as_bytes());
            }
            Ok(tree) => {
                p.field("queries").u8(0).u64(qs.len() as u64);
                for (i, x) in qs.iter().enumerate() {
                    queries += 1;
                    calls[usize::from(x.call)] += 1;
                    put_query(&mut p, &tree, x, i);
                }
            }
        }
        check(m, rec, &label, &p);
    });
    assert!(n > 0, "{}: no PTGT record", file.path.display());
    assert!(queries > 0, "{}: no PTGT query", file.path.display());
    assert!(
        calls.iter().all(|&k| k > 0),
        "{}: a call no record makes: {calls:?}",
        file.path.display()
    );
}

#[test]
fn every_record_kind_is_replayed() {
    let file = ptree_file();
    for rec in &file.records {
        assert!(
            matches!(rec.kind(), "HEAD" | "PRUN" | "PTXR" | "PTGT"),
            "{}: a {} record nothing replays",
            file.path.display(),
            rec.kind()
        );
    }
}
