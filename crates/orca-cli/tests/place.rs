// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The placement (`place.rs`) and the parity knobs (`oracle.rs`), held to the C++ program's
//! bytes: one run of the staged C++ program with oracle patches 0001 to 0006 (2026-11-11,
//! orca-info-place) on the corpus's placement jobs and smoke.ts's placement refusals, each under
//! the golden pass's knobs (`SKETCH_ORCA_ORACLE_TIME` 1767225600, `_SEED` 1, `_THREADS` 1 and a
//! dump folder of its own) unless it set its own.
//!
//! - tests/place/cases/<case>: the job (its paths written `<case>`, `<presets>` and `<resources>`,
//!   common::recorded_job), its model, the C++ program's result and progress file, and for a job
//!   the C++ program placed, its front-objects dump.
//! - A job the C++ program refuses before the front-objects boundary gets its result and progress
//!   file, with nothing on stdout or stderr and nothing dumped: an object's or a height range's
//!   setting it does not take, custom G-code it refuses, a prime tower set past the bed or with no
//!   room, objects that do not fit (the smoke golden refusal-too-big-for-the-bed among them),
//!   filaments too far apart, an object outside the printable volume or partly outside it, and a
//!   plate with no printable object.
//! - A job the C++ program places is placed as it places it: the dump `front-objects-1.sop1` is the
//!   C++ program's byte for byte (several objects arranged, arrange off, a bed exclusion area, a
//!   round bed, the prime tower in its default corner, moved off an excluded area and set by the
//!   job, print by object, custom G-code per layer, an object's settings and height ranges, every
//!   object marked not printable, and the corpus's Bambu project, whose PNTV records carry its
//!   support, seam, colour and fuzzy-skin painting), the RUN record's FMA3 state this processor's.
//!   Then the job is declined, naming every family it reaches past the front-objects boundary
//!   (one list per case: the generic layers job's, with the prime tower and multi-filament for the
//!   tower, custom G-code and Bambu cases, printing by object, an object's gyroid infill and the
//!   Bambu project's painting), after the C++ program's progress lines up to the arrange. The Bambu project's case, model-3mf-bambu-layers, is from one run of the staged
//!   oracle under the same knobs (2026-11-12, orca-o2-review-tests).
//! - The knobs: one the oracle cannot read stops the program with the oracle's line on stderr and
//!   exit 78, nothing written, whichever comes first in the oracle's order (24 runs,
//!   tests/place/knobs/<case>.stderr). A dump under other knobs (THREADS 4, FMA3 0 and 1, the dump
//!   knob alone) has the C++ program's RUN record (tests/place/knobs/<case>.run), with the machine
//!   fields that are the machine's (the default concurrency, the FMA3 state) read from this one,
//!   and the rest of place-several-objects' dump. The dump knob alone's record has its arena's
//!   concurrency equal to its thread limit (TBB's default fills both); the value itself is checked
//!   only on the recording machine.

mod common;

use std::path::{Path, PathBuf};

use common::{
    Scratch, as_recorded_result, program, recorded_job, recorded_stream, repo, write_job,
};
use orca_json::{Object, Order, Value, parse};

/// tests/place.
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("place")
}

const TIME: &str = "SKETCH_ORCA_ORACLE_TIME";
const SEED: &str = "SKETCH_ORCA_ORACLE_SEED";
const THREADS: &str = "SKETCH_ORCA_ORACLE_THREADS";
const FMA3: &str = "SKETCH_ORCA_ORACLE_FMA3";
const DUMP: &str = "SKETCH_ORCA_ORACLE_DUMP";

/// The golden pass's knobs, which a case was run with besides its dump folder unless it set its
/// own.
const PINNED: [(&str, &str); 3] = [(TIME, "1767225600"), (SEED, "1"), (THREADS, "1")];

/// What a run gave.
struct Ran {
    scratch: Scratch,
    code: i32,
    result: Option<String>,
    progress: Option<String>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl Ran {
    /// The files in the dump folder, sorted.
    fn dumped(&self) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(self.scratch.path("dump"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        orca_stl::sort(&mut out);
        out
    }

    /// The dump `front-objects-1.sop1`.
    fn dump(&self) -> Vec<u8> {
        std::fs::read(self.scratch.path("dump/front-objects-1.sop1")).unwrap()
    }

    /// What the run wrote on stdout and stderr, for a failure's message.
    fn streams(&self) -> String {
        format!(
            "stdout {:?}, stderr {:?}",
            String::from_utf8_lossy(&self.stdout),
            String::from_utf8_lossy(&self.stderr)
        )
    }
}

/// Runs the job of the case `id` (tests/place/cases/<id>) as the C++ program was run on it, from a
/// scratch folder named for `name` (`<case>`), with --out and --progress, the variables of `env`
/// (`<case>` in a value made the scratch folder) and, unless they set it, the dump knob naming an
/// empty folder there.
fn run_case(id: &str, name: &str, env: &[(&str, &str)]) -> Ran {
    let scratch = Scratch::new("place", name);
    let job = write_job(
        &scratch,
        &recorded_job(&scratch, &fixtures().join("cases").join(id)),
    );
    std::fs::create_dir_all(scratch.path("dump")).unwrap();
    let here = scratch.0.to_string_lossy().into_owned();
    let mut vars: Vec<(String, String)> = env
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.replace("<case>", &here)))
        .collect();
    if !env.iter().any(|(k, _)| *k == DUMP) {
        vars.push((DUMP.to_string(), scratch.path("dump")));
    }
    let vars: Vec<(&str, &str)> = vars.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let (out, progress) = (scratch.path("result.json"), scratch.path("progress.ndjson"));
    let r = program(
        &["run", "--job", &job, "--out", &out, "--progress", &progress],
        &vars,
    );
    Ran {
        code: r.status.code().unwrap(),
        result: std::fs::read_to_string(&out).ok(),
        progress: std::fs::read_to_string(&progress).ok(),
        stdout: r.stdout,
        stderr: r.stderr,
        scratch,
    }
}

/// A SOP1 file's records: each one's kind and its bytes, its kind and length included.
fn records(file: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    assert_eq!(
        &file[..8],
        b"SOP1\x01\x00\x00\x00",
        "a SOP1 file of version 1"
    );
    let mut at = 8;
    let mut out = Vec::new();
    while at < file.len() {
        let kind: [u8; 4] = file[at..at + 4].try_into().unwrap();
        let n = usize::try_from(u64::from_le_bytes(
            file[at + 4..at + 12].try_into().unwrap(),
        ))
        .unwrap();
        out.push((kind, file[at..at + 12 + n].to_vec()));
        at += 12 + n;
    }
    out
}

/// The RUN record of the SOP1 file `file`.
fn run_record(file: &[u8]) -> Vec<u8> {
    records(file)
        .into_iter()
        .find(|(kind, _)| kind == b"RUN ")
        .unwrap()
        .1
}

/// The SOP1 file `file` with `run` for its RUN record.
fn with_run(file: &[u8], run: &[u8]) -> Vec<u8> {
    let mut out = file[..8].to_vec();
    for (kind, bytes) in records(file) {
        out.extend_from_slice(if &kind == b"RUN " { run } else { &bytes });
    }
    out
}

/// A RUN record with its machine fields, the last 16 bytes (the arena's concurrency, the thread
/// limit and the CRT's FMA3 state), made those given.
fn with_machine(run: &[u8], concurrency: Option<i32>, limit: Option<u64>, fma3: i32) -> Vec<u8> {
    let mut out = run.to_vec();
    let n = out.len();
    if let Some(c) = concurrency {
        out[n - 16..n - 12].copy_from_slice(&c.to_le_bytes());
    }
    if let Some(l) = limit {
        out[n - 12..n - 4].copy_from_slice(&l.to_le_bytes());
    }
    out[n - 4..].copy_from_slice(&fma3.to_le_bytes());
    out
}

/// The CRT's FMA3 state with no knob changing it, which a RUN record reports: the static UCRT's
/// own choice for this processor on Windows x64, -1 elsewhere.
fn fma3_state() -> i32 {
    #[cfg(all(windows, target_env = "msvc", target_arch = "x86_64"))]
    {
        orca_crt::get_fma3_enable()
    }
    #[cfg(not(all(windows, target_env = "msvc", target_arch = "x86_64")))]
    {
        -1
    }
}

/// Where the dump `got` first differs from `want`, for a failure's message: the record, its kind,
/// and the bytes about the first difference, as hex and as text.
fn difference(got: &[u8], want: &[u8]) -> String {
    let (a, b) = (records(got), records(want));
    for (i, ((ka, ra), (kb, rb))) in a.iter().zip(&b).enumerate() {
        if ra != rb {
            let at = ra
                .iter()
                .zip(rb)
                .position(|(x, y)| x != y)
                .unwrap_or(ra.len().min(rb.len()));
            let window = |r: &[u8]| r[at.saturating_sub(24)..(at + 24).min(r.len())].to_vec();
            return format!(
                "record {i} ({} against {}), {} bytes against {}, first difference at byte {at}:\n  got  {:02x?}\n  want {:02x?}\n  got  {:?}\n  want {:?}",
                String::from_utf8_lossy(ka),
                String::from_utf8_lossy(kb),
                ra.len(),
                rb.len(),
                window(ra),
                window(rb),
                String::from_utf8_lossy(&window(ra)),
                String::from_utf8_lossy(&window(rb)),
            );
        }
    }
    format!("{} records against {}", a.len(), b.len())
}

/// The families every placed case reaches past the front-objects boundary: a layers job with the
/// generic settings (decline.rs `command_families` and `setting_families`, corpus.ts
/// `familiesFor`).
const PAST_PLACEMENT: &[&str] = &[
    "slice-mesh",
    "perimeters-arachne",
    "infill-monotonic",
    "infill-monotonicline",
    "infill-crosshatch",
    "infill-concentricinternal",
    "step-estimate-curled-extrusions",
    "step-prepare-infill",
    "step-simplify-path",
    "step-detect-overhangs-for-lift",
    "step-simplify-wall",
    "step-simplify-infill",
    "step-skirt-brim",
    "step-conflict-check",
    "tool-ordering",
    "support-check",
    "layers",
];

/// What a placed case's decline names: [`PAST_PLACEMENT`] with the families its settings, its
/// filaments and its model add (`extra`), in parity.json's order.
fn past_placement(extra: &[&str]) -> Vec<String> {
    let doc = parse(
        &std::fs::read(repo().join("parity/parity.json")).unwrap(),
        Order::Insertion,
    )
    .unwrap();
    let order: Vec<&str> = doc["families"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    for f in PAST_PLACEMENT.iter().chain(extra) {
        assert!(order.contains(f), "parity.json lists {f}");
    }
    order
        .into_iter()
        .filter(|f| PAST_PLACEMENT.contains(f) || extra.contains(f))
        .map(str::to_string)
        .collect()
}

/// Holds the case `id`, which the C++ program placed, to its run: the dump is the C++ program's
/// byte for byte (the RUN record's FMA3 state this processor's), then the job is declined naming
/// `missing` (every family it reaches past the boundary), its progress file the C++ program's
/// lines up to the boundary and the decline, with nothing on stdout or stderr.
fn check_placed(id: &str, missing: &[String]) {
    let ran = run_case(id, id, &PINNED);
    let case = fixtures().join("cases").join(id);
    let result = ran
        .result
        .as_deref()
        .unwrap_or_else(|| panic!("{id}: no result; {}", ran.streams()));
    let r = parse(result.as_bytes(), Order::Insertion).unwrap();
    assert_eq!(
        (ran.code, r["code"].as_str()),
        (-8, Some("not-ported")),
        "{id}: {result}"
    );
    let named: Vec<&str> = r["missing"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap())
        .collect();
    assert_eq!(named, missing, "{id}");
    assert_eq!(ran.dumped(), ["front-objects-1.sop1"], "{id}");
    let want = std::fs::read(case.join("front-objects-1.sop1")).unwrap();
    let want = with_run(
        &want,
        &with_machine(&run_record(&want), None, None, fma3_state()),
    );
    let got = ran.dump();
    assert!(
        got == want,
        "{id}: the dump is not the C++ program's: {}",
        difference(&got, &want)
    );
    // The C++ program's progress lines up to the boundary, then the decline at the last percent.
    let cpp = std::fs::read_to_string(case.join("progress.ndjson")).unwrap();
    let mut progress = String::new();
    let mut last = 0.0;
    for line in cpp.lines() {
        let l = parse(line.as_bytes(), Order::Insertion).unwrap();
        if !matches!(
            l["stage"].as_str(),
            Some("job" | "presets" | "model" | "orient" | "arrange")
        ) {
            break;
        }
        progress.push_str(line);
        progress.push('\n');
        last = l["percent"].as_f64().unwrap();
    }
    let mut failed = Object::new(Order::Insertion);
    failed.insert("stage", "failed");
    failed.insert("percent", last);
    failed.insert("message", r["message"].as_str().unwrap());
    progress.push_str(&Value::Object(failed).dump(None));
    progress.push('\n');
    assert_eq!(ran.progress.as_deref(), Some(progress.as_str()), "{id}");
    assert!(
        ran.stdout.is_empty() && ran.stderr.is_empty(),
        "{id}: {}",
        ran.streams()
    );
}

/// Holds the case `id`, which the C++ program refused before the front-objects boundary, to its
/// run: its result, exit code and progress file, nothing on stdout or stderr, nothing dumped.
/// Returns the result.
fn check_refused(id: &str) -> String {
    let ran = run_case(id, id, &PINNED);
    let case = fixtures().join("cases").join(id);
    let want = std::fs::read_to_string(case.join("result.json")).unwrap();
    let result = ran
        .result
        .clone()
        .unwrap_or_else(|| panic!("{id}: no result; {}", ran.streams()));
    assert_eq!(as_recorded_result(&result, &want), want, "{id}");
    let code = parse(want.as_bytes(), Order::Insertion).unwrap()["exitCode"]
        .as_f64()
        .unwrap();
    assert_eq!(f64::from(ran.code), code, "{id}");
    assert_eq!(
        ran.progress.as_deref(),
        Some(
            std::fs::read_to_string(case.join("progress.ndjson"))
                .unwrap()
                .as_str()
        ),
        "{id}"
    );
    assert!(
        ran.stdout.is_empty() && ran.stderr.is_empty(),
        "{id}: {}",
        ran.streams()
    );
    assert!(ran.dumped().is_empty(), "{id}: nothing is dumped");
    result
}

#[test]
fn several_objects_are_arranged_as_the_cpp_program_arranges_them() {
    check_placed("place-several-objects", &past_placement(&[]));
}

#[test]
fn objects_left_where_they_are_are_placed_as_the_cpp_program_leaves_them() {
    check_placed("place-arrange-off", &past_placement(&[]));
}

#[test]
fn objects_are_arranged_clear_of_the_beds_exclusion_area() {
    check_placed("place-bed-exclude-area", &past_placement(&[]));
}

#[test]
fn objects_are_arranged_on_a_round_bed_as_the_cpp_program_arranges_them() {
    check_placed("place-round-bed", &past_placement(&[]));
}

#[test]
fn the_prime_tower_takes_its_default_corner() {
    check_placed(
        "place-tower-default-corner",
        &past_placement(&["wipe-tower", "multi-filament"]),
    );
}

#[test]
fn the_prime_tower_is_moved_off_an_excluded_area() {
    check_placed(
        "place-tower-off-excluded-area",
        &past_placement(&["wipe-tower", "multi-filament"]),
    );
}

#[test]
fn the_prime_tower_stays_where_the_job_sets_it() {
    check_placed(
        "place-tower-set-by-job",
        &past_placement(&["wipe-tower", "multi-filament"]),
    );
}

#[test]
fn objects_printed_one_by_one_are_arranged_as_the_cpp_program_arranges_them() {
    check_placed("place-by-object", &past_placement(&["print-by-object"]));
}

#[test]
fn the_custom_gcode_per_layer_is_set_as_the_cpp_program_sets_it() {
    check_placed(
        "place-custom-gcodes",
        &past_placement(&["wipe-tower", "multi-filament"]),
    );
}

#[test]
fn an_objects_settings_and_height_ranges_are_set_as_the_cpp_program_sets_them() {
    check_placed("place-object-settings", &past_placement(&["infill-gyroid"]));
}

#[test]
fn objects_all_marked_not_printable_are_placed_as_the_cpp_program_places_them() {
    check_placed("place-every-object-not-printable", &past_placement(&[]));
}

#[test]
fn a_painted_3mf_projects_painting_is_placed_as_the_cpp_program_places_it() {
    // The corpus's Bambu project paints object_1's triangles (supports, seam, colour, fuzzy skin):
    // its PNTV records have content, and its BRIM records are empty (it has no brim ears). The
    // fixture keeps a PNTV record longer than any of an unpainted job's, so the case holds content.
    let pntv = |id: &str| -> Vec<usize> {
        let file = fixtures()
            .join("cases")
            .join(id)
            .join("front-objects-1.sop1");
        records(&std::fs::read(file).unwrap())
            .into_iter()
            .filter(|(kind, _)| kind == b"PNTV")
            .map(|(_, bytes)| bytes.len())
            .collect()
    };
    let unpainted = pntv("place-several-objects");
    assert!(
        pntv("model-3mf-bambu-layers")
            .into_iter()
            .any(|n| unpainted.iter().all(|u| n > *u)),
        "the Bambu project's dump has painting"
    );
    check_placed(
        "model-3mf-bambu-layers",
        &past_placement(&["wipe-tower", "multi-filament", "painted-facets"]),
    );
}

#[test]
fn an_objects_or_a_height_ranges_setting_it_does_not_take_is_refused_as_the_cpp_program_refuses_it()
{
    for id in [
        "object-unknown-setting",
        "object-whole-plate-setting",
        "object-extruder-past-filaments",
        "range-object-setting",
        "range-zero-layer-height",
        "range-extruder-past-filaments",
    ] {
        check_refused(id);
    }
}

#[test]
fn custom_gcode_it_does_not_take_is_refused_as_the_cpp_program_refuses_it() {
    for id in [
        "color-change-without-extruder",
        "filament-change-one-filament",
        "filament-change-past-filaments",
        "pause-without-pause-gcode",
        "custom-gcode-by-object",
    ] {
        check_refused(id);
    }
}

#[test]
fn a_prime_tower_set_past_the_bed_or_with_no_room_is_refused_as_the_cpp_program_refuses_it() {
    for id in ["tower-set-past-the-bed", "tower-fits-nowhere"] {
        check_refused(id);
    }
}

#[test]
fn objects_that_do_not_fit_or_cannot_share_the_plate_are_refused_as_the_cpp_program_refuses_them() {
    for id in ["place-objects-do-not-fit", "place-temperature-difference"] {
        check_refused(id);
    }
    // The smoke refusal too-big-for-the-bed: its golden is the C++ program's result as well.
    let result = check_refused("too-big-for-the-bed");
    let golden = std::fs::read_to_string(
        repo().join("parity/goldens/front/intake/refusal-too-big-for-the-bed.result.json"),
    )
    .unwrap();
    assert_eq!(as_recorded_result(&result, &golden), golden);
}

#[test]
fn objects_outside_the_printable_volume_are_refused_as_the_cpp_program_refuses_them() {
    for id in [
        "off-the-bed",
        "place-arrange-off-partly-outside",
        "place-no-object-inside",
    ] {
        check_refused(id);
    }
}

/// The knob refusals of the C++ program's run (tests/place/knobs/<case>.stderr), each with the
/// knobs it was run with; the dump knob, unless set here, named an empty folder.
const KNOB_REFUSALS: [(&str, &[(&str, &str)]); 24] = [
    ("k-fma3-2", &[(FMA3, "2")]),
    ("k-fma3-empty", &[(FMA3, "")]),
    ("k-fma3-00", &[(FMA3, "00")]),
    ("k-threads-0", &[(THREADS, "0")]),
    ("k-threads-257", &[(THREADS, "257")]),
    ("k-threads-0001", &[(THREADS, "0001")]),
    ("k-threads-1a", &[(THREADS, "1a")]),
    ("k-threads-empty", &[(THREADS, "")]),
    ("k-time-low", &[(TIME, "315532799")]),
    ("k-time-high", &[(TIME, "4102444800")]),
    ("k-time-11-digits", &[(TIME, "01767225600")]),
    ("k-time-plus", &[(TIME, "+1767225600")]),
    ("k-time-empty", &[(TIME, "")]),
    ("k-seed-neg", &[(SEED, "-1")]),
    ("k-seed-big", &[(SEED, "4294967296")]),
    ("k-seed-empty", &[(SEED, "")]),
    ("k-dump-empty", &[(DUMP, "")]),
    ("k-dump-missing", &[(DUMP, "<case>\\no-such-folder")]),
    ("k-dump-file", &[(DUMP, "<case>\\job.json")]),
    (
        "k-order-fma3-threads",
        &[(FMA3, "2"), (THREADS, "0"), (TIME, "1")],
    ),
    (
        "k-order-threads-time",
        &[(THREADS, "0"), (TIME, "1"), (SEED, "x")],
    ),
    ("k-order-time-seed", &[(TIME, "1"), (SEED, "x"), (DUMP, "")]),
    ("k-order-seed-dump", &[(SEED, "x"), (DUMP, "")]),
    (
        "k-order-threads-ok-time-bad",
        &[(THREADS, "2"), (TIME, "1")],
    ),
];

#[test]
fn a_knob_the_oracle_cannot_read_stops_the_program_as_it_stops_the_oracle() {
    for (id, knobs) in KNOB_REFUSALS {
        let ran = run_case("place-several-objects", id, knobs);
        let here = ran.scratch.0.to_string_lossy().into_owned();
        let want = std::fs::read_to_string(fixtures().join("knobs").join(format!("{id}.stderr")))
            .unwrap()
            .replace("<case>", &here);
        assert_eq!(ran.code, 78, "{id}: {}", ran.streams());
        assert_eq!(
            String::from_utf8_lossy(&ran.stderr),
            recorded_stream(&want),
            "{id}"
        );
        assert!(ran.stdout.is_empty(), "{id}: {}", ran.streams());
        assert!(
            ran.result.is_none() && ran.progress.is_none(),
            "{id}: nothing is written"
        );
        assert!(ran.dumped().is_empty(), "{id}: nothing is dumped");
    }
}

/// Holds a run of place-several-objects' job under `knobs` to the C++ program's run of it: its RUN
/// record is tests/place/knobs/<id>.run with the machine fields given, the rest of its dump is
/// place-several-objects', and it is declined as that job is.
fn check_knob_dump(
    id: &str,
    knobs: &[(&str, &str)],
    concurrency: Option<i32>,
    limit: Option<u64>,
    fma3: i32,
) {
    let ran = run_case("place-several-objects", id, knobs);
    let result = ran
        .result
        .as_deref()
        .unwrap_or_else(|| panic!("{id}: no result; {}", ran.streams()));
    assert_eq!(ran.code, -8, "{id}: {result}");
    assert_eq!(ran.dumped(), ["front-objects-1.sop1"], "{id}");
    let base =
        std::fs::read(fixtures().join("cases/place-several-objects/front-objects-1.sop1")).unwrap();
    let run = std::fs::read(fixtures().join("knobs").join(format!("{id}.run"))).unwrap();
    let want = with_run(&base, &with_machine(&run, concurrency, limit, fma3));
    let got = ran.dump();
    assert!(
        got == want,
        "{id}: the dump is not the C++ program's: {}",
        difference(&got, &want)
    );
    assert!(
        ran.stdout.is_empty() && ran.stderr.is_empty(),
        "{id}: {}",
        ran.streams()
    );
}

#[test]
fn a_dump_reports_the_knobs_and_the_threads_as_the_oracle_reports_them() {
    // Four threads: the arena's concurrency is 4, the thread limit 1.
    check_knob_dump(
        "knob-threads-4",
        &[(TIME, "1767225600"), (SEED, "1"), (THREADS, "4")],
        None,
        None,
        fma3_state(),
    );
    // FMA3 1: the CRT's own choice, with the knob reported as set.
    check_knob_dump(
        "knob-fma3-1",
        &[
            (TIME, "1767225600"),
            (SEED, "1"),
            (THREADS, "1"),
            (FMA3, "1"),
        ],
        None,
        None,
        fma3_state(),
    );
    // The dump knob alone: no knob reported, the arena's concurrency and the thread limit the
    // default concurrency, which the C++ program's run had on this machine's processors. TBB's
    // default fills both fields, so the recorded record has the two equal; the value itself is
    // patched in from this machine, so it is checked only on the recording machine.
    let recorded = std::fs::read(fixtures().join("knobs/knob-dump-only.run")).unwrap();
    let n = recorded.len();
    let concurrency = i32::from_le_bytes(recorded[n - 16..n - 12].try_into().unwrap());
    let limit = u64::from_le_bytes(recorded[n - 12..n - 4].try_into().unwrap());
    assert_eq!(
        u64::try_from(concurrency).ok(),
        Some(limit),
        "knob-dump-only.run: the arena's concurrency is its thread limit"
    );
    let default = orca_tbb::default_concurrency();
    check_knob_dump(
        "knob-dump-only",
        &[],
        Some(i32::try_from(default).unwrap()),
        Some(u64::try_from(default).unwrap()),
        fma3_state(),
    );
}

#[test]
fn the_fma3_knob_turns_the_crts_fma3_paths_off_where_the_crt_has_the_switch() {
    let knobs = [
        (TIME, "1767225600"),
        (SEED, "1"),
        (THREADS, "1"),
        (FMA3, "0"),
    ];
    if cfg!(all(windows, target_env = "msvc", target_arch = "x86_64")) {
        check_knob_dump("knob-fma3-0", &knobs, None, None, 0);
    } else {
        // The oracle's refusal where the CRT has no switch (oracle patch 0004).
        let ran = run_case("place-several-objects", "knob-fma3-0", &knobs);
        assert_eq!(ran.code, 78, "{}", ran.streams());
        assert_eq!(
            String::from_utf8_lossy(&ran.stderr),
            "sketch-orca-oracle: SKETCH_ORCA_ORACLE_FMA3=0 works only in the Windows x64 build, whose CRT has the switch\n"
        );
        assert!(ran.result.is_none() && ran.dumped().is_empty());
    }
}
