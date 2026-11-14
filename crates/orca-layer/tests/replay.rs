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
// Carries no OrcaSlicer code: it replays the dumps the parity oracle's OracleDump.cpp wrote into
// the goldens (parity/README.md, parity/SOP1.md). Written on 2026-11-14 (orca-o3-dump-1).

//! The dump gate for orca-layer, over the goldens `SKETCH_ORCA_GOLDENS_DEEP` names (a skip, which
//! is no pass, when it is not set):
//!
//! - Every front-config dump of every domain-A run, read into the data model over its
//!   front-objects' model and written again ([`orca_layer::dump`]), gives the recorded bytes.
//! - Every object dump of those runs, read into its front-config's print and written again, gives
//!   the recorded bytes.
//! - Every dump of those runs (print dumps, front-objects, front-config and object dumps) read
//!   record by record gives back each record's kind, and fields that write the recorded payload.
//!
//! A run is of domain A when its case (parity/corpus.json) has no family of `MARKER_FAMILIES`
//! (scripts/lib/parity/corpus.ts), which this test mirrors and checks against that file's text.
//! front-config-n reads over the front-objects of the highest n' <= n; an object dump of count k
//! reads into the print of the front-config of the highest n <= k.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use orca_layer::replay::{
    FrontConfig, GoldenRun, first_difference, golden_runs, goldens_root, parse_dump_name,
    read_container, read_front_config, read_front_objects, read_object_dump, read_records,
    write_front_config, write_object_dump,
};

/// The families of `MARKER_FAMILIES` named one by one (the rest are the infill families of the
/// patterns outside domain A).
const NAMED_MARKERS: [&str; 12] = [
    "supports",
    "tree-supports",
    "raft",
    "ironing",
    "fuzzy-skin",
    "wipe-tower",
    "step-contouring",
    "step-simplify-support-path",
    "multi-filament",
    "painted-facets",
    "print-by-object",
    "fill-multiline",
];

/// `DOMAIN_A_PATTERNS`.
const DOMAIN_A_PATTERNS: [&str; 6] = [
    "monotonic",
    "monotonicline",
    "rectilinear",
    "crosshatch",
    "concentric",
    "concentricinternal",
];

/// The line of `MARKER_FAMILIES` that adds the infill families outside domain A.
const INFILL_MARKERS: &str = "...INFILL_PATTERNS.filter((p) => !(DOMAIN_A_PATTERNS as readonly \
                              string[]).includes(p)).map((p) => `infill-${p}`),";

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The text between `start` and the first `end` after it.
fn block<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    let from = text
        .find(start)
        .unwrap_or_else(|| panic!("corpus.ts has no {start:?}"))
        + start.len();
    let len = text[from..]
        .find(end)
        .unwrap_or_else(|| panic!("corpus.ts does not close {start:?}"));
    &text[from..from + len]
}

/// The single-quoted strings of a text, in their order.
fn quoted(text: &str) -> Vec<String> {
    text.split('\'')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// `MARKER_FAMILIES`, from the mirror, after checking the mirror against corpus.ts.
fn marker_families() -> BTreeSet<String> {
    let path = repo().join("scripts/lib/parity/corpus.ts");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let infill = quoted(block(
        &text,
        "export const INFILL_PATTERNS = [",
        "] as const",
    ));
    assert!(!infill.is_empty(), "corpus.ts lists no INFILL_PATTERNS");
    let domain = quoted(block(
        &text,
        "export const DOMAIN_A_PATTERNS = [",
        "] as const",
    ));
    assert_eq!(
        domain, DOMAIN_A_PATTERNS,
        "DOMAIN_A_PATTERNS moved in corpus.ts"
    );
    let markers = block(&text, "export const MARKER_FAMILIES: string[] = [", "\n]");
    let mut lines = markers.lines().map(str::trim).filter(|l| !l.is_empty());
    assert_eq!(
        lines.next(),
        Some(INFILL_MARKERS),
        "MARKER_FAMILIES no longer starts with the infill families outside domain A"
    );
    let named: Vec<String> = lines.flat_map(quoted).collect();
    assert_eq!(named, NAMED_MARKERS, "MARKER_FAMILIES moved in corpus.ts");
    infill
        .iter()
        .filter(|p| !DOMAIN_A_PATTERNS.contains(&p.as_str()))
        .map(|p| format!("infill-{p}"))
        .chain(NAMED_MARKERS.iter().map(|&m| m.to_owned()))
        .collect()
}

/// Each case of corpus.json with whether it is of domain A.
fn domain_a_cases() -> BTreeMap<String, bool> {
    let markers = marker_families();
    let path = repo().join("parity/corpus.json");
    let text = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let corpus = orca_json::parse(&text, orca_json::Order::Insertion)
        .unwrap_or_else(|e| panic!("corpus.json: {e:?}"));
    let cases = corpus
        .get("cases")
        .and_then(orca_json::Value::as_array)
        .expect("corpus.json lists its cases");
    cases
        .iter()
        .map(|case| {
            let id = case
                .get("id")
                .and_then(orca_json::Value::as_str)
                .expect("a case has an id");
            let families = case
                .get("families")
                .and_then(orca_json::Value::as_array)
                .expect("a case lists its families");
            let outside = families
                .iter()
                .filter_map(orca_json::Value::as_str)
                .any(|f| markers.contains(f));
            (id.to_owned(), !outside)
        })
        .collect()
}

#[test]
fn the_marker_mirror_is_corpus_ts() {
    let markers = marker_families();
    assert!(markers.contains("infill-gyroid"));
    assert!(!markers.contains("infill-rectilinear"));
    assert!(markers.contains("wipe-tower"));
}

/// What the replay compared and what failed.
#[derive(Default)]
struct Tally {
    runs: usize,
    front_configs: usize,
    object_dumps: usize,
    record_dumps: usize,
    records: usize,
    failures: Vec<String>,
}

/// The value of `map` at the highest key <= `k`, else at its lowest key.
fn at_or_below<V>(map: &BTreeMap<u32, V>, k: u32) -> Option<(u32, &V)> {
    map.range(..=k)
        .next_back()
        .or_else(|| map.iter().next())
        .map(|(&n, v)| (n, v))
}

/// Every dump of a run read record by record.
fn replay_records(run: &GoldenRun, files: &[String], tally: &mut Tally) {
    for file in files {
        let at = format!("{}/{}/{file}", run.case, run.run);
        let bytes = match run.read(file) {
            Ok(b) => b,
            Err(e) => {
                tally.failures.push(format!("{at}: {e}"));
                continue;
            }
        };
        let (raw, records) = match (read_container(&bytes), read_records(&bytes)) {
            (Ok(raw), Ok(records)) => (raw, records),
            (Err(e), _) | (_, Err(e)) => {
                tally.failures.push(format!("{at}: records: {e}"));
                continue;
            }
        };
        tally.record_dumps += 1;
        for (i, (raw, record)) in raw.iter().zip(&records).enumerate() {
            tally.records += 1;
            if raw.kind != record.kind || record.payload() != raw.payload {
                tally.failures.push(format!(
                    "{at}: record {i} ({}) does not read back as its kind and fields",
                    raw.kind_name()
                ));
                break;
            }
        }
    }
}

/// A run's front-config and object dumps read into the data model and written again.
fn replay_state(run: &GoldenRun, files: &[String], tally: &mut Tally) {
    let mut objects_files = BTreeMap::new();
    let mut config_files = BTreeMap::new();
    let mut object_files = Vec::new();
    for file in files {
        let Some(name) = parse_dump_name(file) else {
            tally.failures.push(format!(
                "{}/{}/{file}: not a dump's name",
                run.case, run.run
            ));
            continue;
        };
        match (name.source.as_str(), name.name.as_str()) {
            ("front", "objects") => {
                objects_files.insert(name.occurrence, file.clone());
            }
            ("front", "config") => {
                config_files.insert(name.occurrence, file.clone());
            }
            ("object", _) => object_files.push((name.occurrence, file.clone())),
            _ => {}
        }
    }
    let mut prints: BTreeMap<u32, FrontConfig> = BTreeMap::new();
    for (&n, file) in &config_files {
        let at = format!("{}/{}/{file}", run.case, run.run);
        let Some((_, objects_file)) = at_or_below(&objects_files, n) else {
            tally.failures.push(format!("{at}: no front-objects dump"));
            continue;
        };
        let read = run
            .read(objects_file)
            .and_then(|b| read_front_objects(&b).map_err(|e| e.context(objects_file)))
            .and_then(|objects| {
                let bytes = run.read(file)?;
                let front = read_front_config(&bytes, objects.model)?;
                let out = write_front_config(&front)?;
                Ok((bytes, front, out))
            });
        match read {
            Ok((bytes, front, out)) => {
                tally.front_configs += 1;
                if let Some(d) = first_difference(&bytes, &out) {
                    tally.failures.push(format!("{at}: written again, {d}"));
                }
                prints.insert(n, front);
            }
            Err(e) => tally.failures.push(format!("{at}: {e}")),
        }
    }
    for (k, file) in object_files {
        let at = format!("{}/{}/{file}", run.case, run.run);
        let Some(n) = at_or_below(&prints, k).map(|(n, _)| n) else {
            tally
                .failures
                .push(format!("{at}: no front-config print to read it into"));
            continue;
        };
        let Some(front) = prints.get_mut(&n) else {
            continue;
        };
        let bytes = match run.read(&file) {
            Ok(b) => b,
            Err(e) => {
                tally.failures.push(format!("{at}: {e}"));
                continue;
            }
        };
        match read_object_dump(&bytes, &mut front.print) {
            Ok(dump) => {
                tally.object_dumps += 1;
                let out = write_object_dump(&front.print, &dump);
                if let Some(d) = first_difference(&bytes, &out) {
                    tally.failures.push(format!("{at}: written again, {d}"));
                }
            }
            Err(e) => tally.failures.push(format!("{at}: {e}")),
        }
    }
}

/// A line written to the process's stdout past the test harness's capture, so that the gate's
/// output shows it whether the test passes or not.
fn report(line: &str) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

#[test]
fn domain_a_dumps_replay_byte_for_byte() {
    let Some(root) = goldens_root() else {
        report(
            "SKIP (no pass): SKETCH_ORCA_GOLDENS_DEEP is not set, so no golden dump was replayed",
        );
        return;
    };
    let cases = domain_a_cases();
    let runs = golden_runs(&root).unwrap_or_else(|e| panic!("{}: {e}", root.display()));
    let mut tally = Tally::default();
    let mut outside = 0usize;
    for run in &runs {
        let Some(&domain_a) = cases.get(&run.case) else {
            tally
                .failures
                .push(format!("{}: a case corpus.json does not list", run.case));
            continue;
        };
        if !run.has("front-config-1.sop1") {
            continue;
        }
        if !domain_a {
            outside += 1;
            continue;
        }
        let files = match run.files() {
            Ok(f) => f,
            Err(e) => {
                tally
                    .failures
                    .push(format!("{}/{}: {e}", run.case, run.run));
                continue;
            }
        };
        tally.runs += 1;
        replay_records(run, &files, &mut tally);
        replay_state(run, &files, &mut tally);
    }
    report(&format!(
        "replayed {} domain-A runs of {} with dumps ({outside} outside domain A left out): {} \
         front-config and {} object dumps read and written again, {} dumps ({} records) read \
         record by record; {} failures",
        tally.runs,
        runs.len(),
        tally.front_configs,
        tally.object_dumps,
        tally.record_dumps,
        tally.records,
        tally.failures.len()
    ));
    for failure in tally.failures.iter().take(40) {
        report(&format!("  {failure}"));
    }
    assert!(
        tally.runs > 0,
        "no domain-A run with a front-config dump under {}",
        root.display()
    );
    assert!(
        tally.failures.is_empty(),
        "{} dumps did not replay; the first: {}",
        tally.failures.len(),
        tally.failures[0]
    );
}
