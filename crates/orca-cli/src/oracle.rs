// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The parity knobs, for parity work only: sketch-orca-rs reads the oracle's knobs
//! (`SKETCH_ORCA_ORACLE_*`, `parity/oracle/README.md`, oracle patches 0001 to 0006) as the oracle
//! reads them, so that one job run under the same knobs can be compared with the oracle's run
//! byte for byte, its dumps included. The C++ engine that ships has no knobs, and nothing here
//! changes what sketch-orca-rs does when no knob is set.
//!
//! - A knob set to something the oracle cannot read stops the program before it does anything
//!   else: `sketch-orca-oracle: <what is wrong>` on stderr and exit 78 ([`knob_error`]), nothing
//!   written. `main` checks `SKETCH_ORCA_ORACLE_FMA3`, then `SKETCH_ORCA_ORACLE_THREADS`
//!   ([`main_problem`]); the engine's main, inside the thread arena when there is one, checks
//!   `SKETCH_ORCA_ORACLE_TIME`, `SKETCH_ORCA_ORACLE_SEED` and `SKETCH_ORCA_ORACLE_DUMP`
//!   ([`engine_problem`]), in that order, as the oracle's two mains do.
//! - `SKETCH_ORCA_ORACLE_FMA3=0` turns the CRT's FMA3 paths off first of all ([`fma3_switch`]),
//!   which in the oracle a static initializer does before every other; only a Windows x64 MSVC
//!   build takes it.
//! - `SKETCH_ORCA_ORACLE_THREADS=P` runs the engine's main in an arena of concurrency P on the
//!   main thread ([`in_thread_arena`]; orca-tbb's serial arena is the oracle's arena under
//!   `global_control(max_allowed_parallelism, 1)`).
//! - `SKETCH_ORCA_ORACLE_TIME` and `SKETCH_ORCA_ORACLE_SEED` are checked and reported in the
//!   dumps' RUN record only: up to the front-objects boundary nothing sketch-orca-rs writes reads
//!   the clock or draws from the generators the seed fixes. The 3MF reader's backup folder in the
//!   work folder is named by the real local time (orca-model), where the oracle names it by the
//!   pinned time; that folder is in no result and no dump.
//! - `SKETCH_ORCA_ORACLE_DUMP` names a folder, read once; with it set, [`dump_front_objects`]
//!   writes `front-objects-<n>.sop1` there once the objects are placed, every record of oracle
//!   patches 0005 and 0006 as `OracleDump.cpp` writes them (`parity/SOP1.md`): HEAD, RUN, the
//!   run's CONF, PCGC, and per object MODL, its CONF and its volumes', HRNG, its height ranges'
//!   CONF, BRIM and its volumes' PNTV, then END. The RUN record reports the knobs as set, the
//!   arena's concurrency, the thread limit (1 under the thread knob, else the default
//!   concurrency) and the CRT's FMA3 state (-1 off Windows x64).
//!
//! The knobs' texts are read as Rust reads the environment, UTF-8; the oracle's `getenv` reads
//! them in the ANSI code page, so a knob with non-ASCII text can read otherwise
//! (`parity/HAZARDS.md`).

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};

use orca_config::OptionValue;
use orca_eigen::Transform3d;
use orca_model::facets::FacetsAnnotation;
use orca_model::model::Model;

use crate::common::{Failure, fail};
use crate::config::Config;
use crate::paths::is_directory;
use crate::place::PlatesCustomGcodes;
use crate::run::join_path;

/// `EXIT_ORACLE_KNOB`: the exit code of a knob the oracle cannot read.
pub const EXIT_ORACLE_KNOB: i32 = 78;

const TIME: &str = "SKETCH_ORCA_ORACLE_TIME";
const SEED: &str = "SKETCH_ORCA_ORACLE_SEED";
const THREADS: &str = "SKETCH_ORCA_ORACLE_THREADS";
const FMA3: &str = "SKETCH_ORCA_ORACLE_FMA3";
const DUMP: &str = "SKETCH_ORCA_ORACLE_DUMP";

const TIME_PROBLEM: &str = "SKETCH_ORCA_ORACLE_TIME must be seconds since 1970-01-01 00:00:00 UTC, a whole number from 315532800 (1980-01-01) to 4102444799 (2099-12-31 23:59:59), with nothing else in it";
const SEED_PROBLEM: &str =
    "SKETCH_ORCA_ORACLE_SEED must be a whole number from 0 to 4294967295, with nothing else in it";
const THREADS_PROBLEM: &str =
    "SKETCH_ORCA_ORACLE_THREADS must be a whole number from 1 to 256, with nothing else in it";
const FMA3_PROBLEM: &str =
    "SKETCH_ORCA_ORACLE_FMA3 must be 0 (the CRT's FMA3 paths off) or 1 (the CRT's own choice)";
const FMA3_NO_SWITCH: &str =
    "SKETCH_ORCA_ORACLE_FMA3=0 works only in the Windows x64 build, whose CRT has the switch";

/// `SKETCH_ORCA_ORACLE_HAS_FMA3_SWITCH`: the CRT's `_set_FMA3_enable` is there (`_MSC_VER` and
/// `_M_X64`).
const HAS_FMA3_SWITCH: bool = cfg!(all(windows, target_env = "msvc", target_arch = "x86_64"));

/// `std::getenv(name)`: the knob's text, `None` when it is not set.
fn knob(name: &str) -> Option<String> {
    std::env::var_os(name).map(|v| v.to_string_lossy().into_owned())
}

/// A knob as the oracle's readers return it: 0 not set, 1 set (with its value), -1 set to
/// something they cannot read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Knob {
    Unset,
    Set(u64),
    Unreadable,
}

/// `read_threads`, `sketch_orca_oracle_time` and `read_seed`: a whole number of at most
/// `max_digits` decimal digits with nothing else in it (no sign, no space), from `lo` to `hi`.
fn read_number(text: Option<&str>, max_digits: usize, lo: u64, hi: u64) -> Knob {
    let Some(text) = text else {
        return Knob::Unset;
    };
    let mut value: u64 = 0;
    let mut digits = 0;
    for c in text.bytes() {
        if !c.is_ascii_digit() {
            return Knob::Unreadable;
        }
        digits += 1;
        if digits > max_digits {
            return Knob::Unreadable;
        }
        value = value * 10 + u64::from(c - b'0');
    }
    if digits == 0 || value < lo || value > hi {
        return Knob::Unreadable;
    }
    Knob::Set(value)
}

/// `read_threads`: P from 1 to 256, in at most three digits.
fn read_threads(text: Option<&str>) -> Knob {
    read_number(text, 3, 1, 256)
}

/// `sketch_orca_oracle_time`: seconds from 315532800 to 4102444799, in at most ten digits.
fn read_time(text: Option<&str>) -> Knob {
    read_number(text, 10, 315_532_800, 4_102_444_799)
}

/// `read_seed`: a seed from 0 to 4294967295, in at most ten digits.
fn read_seed(text: Option<&str>) -> Knob {
    read_number(text, 10, 0, 4_294_967_295)
}

/// `read_fma3_knob`: exactly "0" (the CRT's FMA3 paths off) or "1" (its own choice).
fn read_fma3(text: Option<&str>) -> Knob {
    match text {
        None => Knob::Unset,
        Some("0") => Knob::Set(0),
        Some("1") => Knob::Set(1),
        Some(_) => Knob::Unreadable,
    }
}

/// `fma3_knob_problem`.
fn fma3_problem(text: Option<&str>) -> Option<&'static str> {
    match read_fma3(text) {
        Knob::Unreadable => Some(FMA3_PROBLEM),
        Knob::Set(0) if !HAS_FMA3_SWITCH => Some(FMA3_NO_SWITCH),
        _ => None,
    }
}

/// `threads_knob_problem`.
fn threads_problem(text: Option<&str>) -> Option<&'static str> {
    (read_threads(text) == Knob::Unreadable).then_some(THREADS_PROBLEM)
}

/// `Fma3Switch`: with `SKETCH_ORCA_ORACLE_FMA3=0`, `_set_FMA3_enable(0)`. The oracle does it in a
/// static initializer before every other; Rust runs nothing of the program's before `main`, so
/// `main` calls this first.
pub fn fma3_switch() {
    #[cfg(all(windows, target_env = "msvc", target_arch = "x86_64"))]
    {
        if read_fma3(knob(FMA3).as_deref()) == Knob::Set(0) {
            orca_crt::set_fma3_enable(0);
        }
    }
}

/// What `main` refuses first: `SKETCH_ORCA_ORACLE_FMA3`, then `SKETCH_ORCA_ORACLE_THREADS`.
#[must_use]
pub fn main_problem() -> Option<&'static str> {
    fma3_problem(knob(FMA3).as_deref()).or_else(|| threads_problem(knob(THREADS).as_deref()))
}

/// `threads_set()` and `threads()`: P when `SKETCH_ORCA_ORACLE_THREADS` sets it.
#[must_use]
pub fn threads() -> Option<usize> {
    match read_threads(knob(THREADS).as_deref()) {
        Knob::Set(p) => usize::try_from(p).ok(),
        Knob::Unset | Knob::Unreadable => None,
    }
}

/// Set once `main` runs the engine in the thread knob's arena: TBB's
/// `global_control::active_value(max_allowed_parallelism)` is 1 from then on.
static ONE_THREAD: AtomicBool = AtomicBool::new(false);

/// `main` under the thread knob: `global_control(max_allowed_parallelism, 1)` and
/// `task_arena{P, P}.execute(f)`, `f` run on this thread in an arena of concurrency P.
pub fn in_thread_arena<T: Send>(p: usize, f: impl FnOnce() -> T + Send) -> T {
    ONE_THREAD.store(true, Ordering::SeqCst);
    orca_tbb::Arena::serial(p).execute(f)
}

/// The dump knob, read once (`DumpKnob`): the folder when it is one, else what is wrong.
#[derive(Debug, Default)]
struct DumpKnob {
    folder: Option<String>,
    problem: Option<String>,
}

/// `read_dump_knob`.
fn read_dump_knob(text: Option<&str>) -> DumpKnob {
    let Some(text) = text else {
        return DumpKnob::default();
    };
    if text.is_empty() {
        return DumpKnob {
            folder: None,
            problem: Some(
                "SKETCH_ORCA_ORACLE_DUMP must name an existing folder; it is empty".to_string(),
            ),
        };
    }
    if !is_directory(text) {
        return DumpKnob {
            folder: None,
            problem: Some(format!(
                "SKETCH_ORCA_ORACLE_DUMP must name an existing folder; {text} is not one"
            )),
        };
    }
    DumpKnob {
        folder: Some(text.to_string()),
        problem: None,
    }
}

/// `dump_knob()`: the knob as first read.
fn dump_knob() -> &'static DumpKnob {
    static KNOB: OnceLock<DumpKnob> = OnceLock::new();
    KNOB.get_or_init(|| read_dump_knob(knob(DUMP).as_deref()))
}

/// What the engine's main refuses first: `SKETCH_ORCA_ORACLE_TIME`, `SKETCH_ORCA_ORACLE_SEED`,
/// then `SKETCH_ORCA_ORACLE_DUMP`.
#[must_use]
pub fn engine_problem() -> Option<String> {
    if read_time(knob(TIME).as_deref()) == Knob::Unreadable {
        return Some(TIME_PROBLEM.to_string());
    }
    if read_seed(knob(SEED).as_deref()) == Knob::Unreadable {
        return Some(SEED_PROBLEM.to_string());
    }
    dump_knob().problem.clone()
}

/// `oracle_knob_error(problem)`: "sketch-orca-oracle: <problem>" on stderr, and 78.
pub fn knob_error(err: &mut dyn Write, problem: &str) -> i32 {
    crate::write_text(err, &format!("sketch-orca-oracle: {problem}\n"));
    EXIT_ORACLE_KNOB
}

// ---------------------------------------------------------------------------------------------
// The dumps (OracleDump.cpp)

/// `SOP1_VERSION`.
const SOP1_VERSION: u32 = 1;

/// A record's payload, built in memory; every value little-endian, floats as their bits.
#[derive(Default)]
struct Payload {
    bytes: Vec<u8>,
}

impl Payload {
    fn u8(&mut self, v: u8) {
        self.bytes.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.u32(v as u32);
    }
    fn i64(&mut self, v: i64) {
        self.u64(v as u64);
    }
    fn boolean(&mut self, v: bool) {
        self.u8(u8::from(v));
    }
    fn f32(&mut self, v: f32) {
        self.u32(v.to_bits());
    }
    fn f64(&mut self, v: f64) {
        self.u64(v.to_bits());
    }
    /// A text: its length in bytes, then its bytes.
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.bytes.extend_from_slice(s.as_bytes());
    }
    fn count(&mut self, n: usize) {
        self.u64(n as u64);
    }
}

/// `boundary_counts()` behind `dump_mutex()`: how many dumps of each file name were made, held
/// for the whole of a dump.
static BOUNDARY_COUNTS: Mutex<BTreeMap<String, u32>> = Mutex::new(BTreeMap::new());

/// `class Dump`: one SOP1 file, built whole in memory.
struct Dump {
    file: String,
    bytes: Vec<u8>,
    records: u64,
}

impl Dump {
    /// `Dump(source, name, object)`: the file `<source>-<name>-<n>.sop1` (`object<i>-<name>` for
    /// an object), n counting that name's dumps, with its HEAD and RUN records.
    fn new(counts: &mut BTreeMap<String, u32>, source: &str, name: &str, object: i64) -> Dump {
        let file = if source == "object" {
            format!("object{object}-{name}")
        } else {
            format!("{source}-{name}")
        };
        let count = counts.entry(file.clone()).or_insert(0);
        *count = count.wrapping_add(1);
        let occurrence = *count;
        let mut dump = Dump {
            file: format!("{file}-{occurrence}.sop1"),
            bytes: b"SOP1".to_vec(),
            records: 0,
        };
        dump.bytes.extend_from_slice(&SOP1_VERSION.to_le_bytes());
        let mut head = Payload::default();
        head.str(source);
        head.str(name);
        head.i64(object);
        head.u32(occurrence);
        dump.record(b"HEAD", &head);
        dump.record(b"RUN ", &run_record());
        dump
    }

    /// A record: its kind, its payload's length, the payload.
    fn record(&mut self, kind: &[u8; 4], payload: &Payload) {
        self.bytes.extend_from_slice(kind);
        self.bytes
            .extend_from_slice(&(payload.bytes.len() as u64).to_le_bytes());
        self.bytes.extend_from_slice(&payload.bytes);
        self.records += 1;
    }

    /// `write()`: the END record (the number of records before it), then the file in the dump
    /// folder.
    fn write(mut self, folder: &str) -> Result<(), Failure> {
        let mut end = Payload::default();
        end.u64(self.records);
        self.record(b"END ", &end);
        std::fs::write(join_path(folder, &self.file), &self.bytes).map_err(|_| {
            fail(
                "internal-error",
                format!(
                    "sketch-orca-oracle: could not write the dump {} in SKETCH_ORCA_ORACLE_DUMP",
                    self.file
                ),
            )
        })
    }
}

/// `_get_FMA3_enable()` on Windows x64, else -1.
fn fma3_enable() -> i32 {
    #[cfg(all(windows, target_env = "msvc", target_arch = "x86_64"))]
    {
        orca_crt::get_fma3_enable()
    }
    #[cfg(not(all(windows, target_env = "msvc", target_arch = "x86_64")))]
    {
        -1
    }
}

/// RUN: the knobs as set (each with whether it is set and its text), then what TBB and the CRT
/// report at this boundary: the arena's concurrency, the thread limit and the FMA3 state.
fn run_record() -> Payload {
    let mut p = Payload::default();
    let knobs = [TIME, SEED, THREADS, FMA3];
    p.count(knobs.len());
    for name in knobs {
        let value = knob(name);
        p.str(name);
        p.boolean(value.is_some());
        p.str(value.as_deref().unwrap_or(""));
    }
    p.i32(orca_tbb::max_concurrency() as i32);
    p.u64(if ONE_THREAD.load(Ordering::SeqCst) {
        1
    } else {
        orca_tbb::default_concurrency() as u64
    });
    p.i32(fma3_enable());
    p
}

/// `put_matrix`: the 4 x 4 matrix column by column, as Eigen stores it.
fn put_matrix(p: &mut Payload, t: &Transform3d) {
    for c in 0..4 {
        for r in 0..4 {
            p.f64(t.m.at(r, c));
        }
    }
}

/// `put_option_floats`: an option's numbers (a float, percent or float-or-percent; their lists;
/// a point, its list and a 3D point, coordinate by coordinate); none for any other option.
fn put_option_floats(p: &mut Payload, value: &OptionValue) {
    let values: Vec<f64> = match value {
        OptionValue::Float(v) | OptionValue::Percent(v) => vec![*v],
        OptionValue::FloatOrPercent { value, .. } => vec![*value],
        OptionValue::Floats { values, .. } | OptionValue::Percents { values, .. } => values.clone(),
        OptionValue::FloatsOrPercents { values, .. } => values.iter().map(|v| v.0).collect(),
        OptionValue::Point(v) => v.to_vec(),
        OptionValue::Points(v) => v.iter().flatten().copied().collect(),
        OptionValue::Point3(v) => v.to_vec(),
        _ => Vec::new(),
    };
    p.count(values.len());
    for v in values {
        p.f64(v);
    }
}

/// CONF: where the configuration is from, then every option by key: its key, its type, its text
/// and its numbers.
///
/// # Errors
///
/// `internal-error` with the exception of an option that does not serialize.
fn config_record<'a>(
    scope: &str,
    object: i64,
    part: i64,
    options: impl Iterator<Item = (&'a String, &'a OptionValue)>,
) -> Result<Payload, Failure> {
    let options: Vec<(&String, &OptionValue)> = options.collect();
    let mut p = Payload::default();
    p.str(scope);
    p.i64(object);
    p.i64(part);
    p.count(options.len());
    for (key, value) in options {
        p.str(key);
        p.u32(value.ty().code());
        p.str(&value.serialize().map_err(|e| fail("internal-error", e.0))?);
        put_option_floats(&mut p, value);
    }
    Ok(p)
}

/// PCGC: the plates' custom G-code and the current plate.
fn plates_custom_gcodes_record(model: &Model, gcodes: &PlatesCustomGcodes) -> Payload {
    let mut p = Payload::default();
    p.i32(model.curr_plate_index);
    p.count(gcodes.len());
    for (plate, info) in gcodes {
        p.i32(*plate);
        p.i32(info.mode);
        p.count(info.gcodes.len());
        for item in &info.gcodes {
            p.f64(item.print_z);
            p.i32(item.ty);
            p.i32(item.extruder);
            p.str(&item.color);
            p.str(&item.extra);
        }
    }
    p
}

/// `put_painting`: the triangles to split with where their bits start, the bitstream (its length,
/// then its bits eight to a byte, the first in the lowest bit) and the states used.
fn put_painting(p: &mut Payload, facets: &FacetsAnnotation) {
    let data = facets.get_data();
    p.count(data.triangles_to_split.len());
    for m in &data.triangles_to_split {
        p.i32(m.triangle_idx);
        p.i32(m.bitstream_start_idx);
    }
    p.u64(data.bitstream.len() as u64);
    p.count(data.bitstream.len().div_ceil(8));
    for bits in data.bitstream.chunks(8) {
        let mut byte = 0u8;
        for (b, &bit) in bits.iter().enumerate() {
            if bit {
                byte |= 1 << b;
            }
        }
        p.u8(byte);
    }
    p.count(data.used_states.len());
    for &used in &data.used_states {
        p.boolean(used);
    }
}

/// The dump folder when `SKETCH_ORCA_ORACLE_DUMP` names one (`dump_enabled()`).
fn dump_folder() -> Option<&'static str> {
    dump_knob().folder.as_deref()
}

/// `dump_front_objects(model, config)`: with `SKETCH_ORCA_ORACLE_DUMP` set, the objects as placed,
/// in `front-objects-<n>.sop1`: the run's configuration (`run.rs`'s, with the prime tower's
/// place), the plates' custom G-code, and each object: its MODL (name, printable, layer height
/// profile, instances' matrices, volumes' types, matrices and meshes), its configuration and its
/// volumes', its height ranges with theirs, its brim ears and its volumes' painting.
///
/// # Errors
///
/// `internal-error` when an option does not serialize or the file cannot be written.
pub fn dump_front_objects(
    model: &Model,
    cfg: &Config,
    gcodes: &PlatesCustomGcodes,
) -> Result<(), Failure> {
    let Some(folder) = dump_folder() else {
        return Ok(());
    };
    let mut counts = BOUNDARY_COUNTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let mut dump = Dump::new(&mut counts, "front", "objects", -1);
    dump.record(b"CONF", &config_record("run", -1, -1, cfg.iter())?);
    dump.record(b"PCGC", &plates_custom_gcodes_record(model, gcodes));
    for (i, object) in model.objects.iter().enumerate() {
        let index = i as i64;
        let mut modl = Payload::default();
        modl.i64(index);
        modl.str(&object.name);
        modl.boolean(object.printable);
        modl.count(object.layer_height_profile.len());
        for &v in &object.layer_height_profile {
            modl.f64(v);
        }
        modl.count(object.instances.len());
        for instance in &object.instances {
            put_matrix(&mut modl, instance.get_matrix());
            modl.boolean(instance.printable);
        }
        modl.count(object.volumes.len());
        for volume in &object.volumes {
            modl.str(&volume.name);
            modl.i32(volume.volume_type().value());
            put_matrix(&mut modl, volume.get_matrix());
            let its = &volume.mesh().its;
            modl.count(its.vertices.len());
            for v in &its.vertices {
                modl.f32(v.x());
                modl.f32(v.y());
                modl.f32(v.z());
            }
            modl.count(its.indices.len());
            for f in &its.indices {
                modl.i32(f.x());
                modl.i32(f.y());
                modl.i32(f.z());
            }
        }
        dump.record(b"MODL", &modl);
        dump.record(
            b"CONF",
            &config_record("model-object", index, -1, object.config.iter())?,
        );
        for (v, volume) in object.volumes.iter().enumerate() {
            dump.record(
                b"CONF",
                &config_record("model-volume", index, v as i64, volume.config.iter())?,
            );
        }
        let mut hrng = Payload::default();
        hrng.i64(index);
        hrng.count(object.layer_config_ranges.len());
        for ((from, to), _) in &object.layer_config_ranges {
            hrng.f64(*from);
            hrng.f64(*to);
        }
        dump.record(b"HRNG", &hrng);
        for (r, (_, config)) in object.layer_config_ranges.iter().enumerate() {
            dump.record(
                b"CONF",
                &config_record("model-height-range", index, r as i64, config.iter())?,
            );
        }
        let mut brim = Payload::default();
        brim.i64(index);
        brim.count(object.brim_points.len());
        for ear in &object.brim_points {
            brim.f32(ear.pos.x());
            brim.f32(ear.pos.y());
            brim.f32(ear.pos.z());
            brim.f32(ear.head_front_radius);
        }
        dump.record(b"BRIM", &brim);
        for (v, volume) in object.volumes.iter().enumerate() {
            let mut pntv = Payload::default();
            pntv.i64(index);
            pntv.i64(v as i64);
            put_painting(&mut pntv, &volume.supported_facets);
            put_painting(&mut pntv, &volume.seam_facets);
            put_painting(&mut pntv, &volume.mmu_segmentation_facets);
            put_painting(&mut pntv, &volume.fuzzy_skin_facets);
            dump.record(b"PNTV", &pntv);
        }
    }
    dump.write(folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thread_knob_takes_one_to_three_digits_from_1_to_256_and_nothing_else() {
        assert_eq!(read_threads(None), Knob::Unset);
        assert_eq!(read_threads(Some("1")), Knob::Set(1));
        assert_eq!(read_threads(Some("256")), Knob::Set(256));
        assert_eq!(read_threads(Some("001")), Knob::Set(1));
        for bad in ["", "0", "257", "0001", "1a", " 1", "+1", "-1", "\u{e9}"] {
            assert_eq!(read_threads(Some(bad)), Knob::Unreadable, "{bad:?}");
        }
        assert_eq!(threads_problem(Some("0")), Some(THREADS_PROBLEM));
        assert_eq!(threads_problem(Some("8")), None);
        assert_eq!(threads_problem(None), None);
    }

    #[test]
    fn the_clock_and_seed_knobs_take_up_to_ten_digits_in_their_ranges() {
        assert_eq!(read_time(None), Knob::Unset);
        assert_eq!(read_time(Some("315532800")), Knob::Set(315_532_800));
        assert_eq!(read_time(Some("4102444799")), Knob::Set(4_102_444_799));
        assert_eq!(read_time(Some("1767225600")), Knob::Set(1_767_225_600));
        for bad in [
            "",
            "315532799",
            "4102444800",
            "01767225600",
            "+1767225600",
            "1767225600 ",
        ] {
            assert_eq!(read_time(Some(bad)), Knob::Unreadable, "{bad:?}");
        }
        assert_eq!(read_seed(Some("0")), Knob::Set(0));
        assert_eq!(read_seed(Some("0000000000")), Knob::Set(0));
        assert_eq!(read_seed(Some("4294967295")), Knob::Set(4_294_967_295));
        for bad in ["", "-1", "4294967296", "00000000001", "x"] {
            assert_eq!(read_seed(Some(bad)), Knob::Unreadable, "{bad:?}");
        }
    }

    #[test]
    fn the_fma3_knob_is_exactly_0_or_1_and_0_only_where_the_crt_has_the_switch() {
        assert_eq!(fma3_problem(None), None);
        assert_eq!(fma3_problem(Some("1")), None);
        for bad in ["", "2", "00", " 0", "0 "] {
            assert_eq!(fma3_problem(Some(bad)), Some(FMA3_PROBLEM), "{bad:?}");
        }
        assert_eq!(
            fma3_problem(Some("0")),
            if HAS_FMA3_SWITCH {
                None
            } else {
                Some(FMA3_NO_SWITCH)
            }
        );
    }

    #[test]
    fn the_dump_knob_names_an_existing_folder() {
        assert!(read_dump_knob(None).folder.is_none() && read_dump_knob(None).problem.is_none());
        assert_eq!(
            read_dump_knob(Some("")).problem.as_deref(),
            Some("SKETCH_ORCA_ORACLE_DUMP must name an existing folder; it is empty")
        );
        let here = std::env::temp_dir().to_string_lossy().into_owned();
        assert_eq!(read_dump_knob(Some(&here)).folder.as_deref(), Some(&*here));
        let missing = join_path(&here, "orca-cli-oracle-no-such-folder");
        assert_eq!(
            read_dump_knob(Some(&missing)).problem,
            Some(format!(
                "SKETCH_ORCA_ORACLE_DUMP must name an existing folder; {missing} is not one"
            ))
        );
    }

    #[test]
    fn a_knob_error_is_one_line_on_stderr_and_78() {
        let mut err = Vec::new();
        assert_eq!(knob_error(&mut err, THREADS_PROBLEM), 78);
        let newline = if cfg!(windows) { "\r\n" } else { "\n" };
        assert_eq!(
            String::from_utf8(err).unwrap(),
            format!("sketch-orca-oracle: {THREADS_PROBLEM}{newline}")
        );
    }

    #[test]
    fn payloads_are_little_endian_with_texts_and_counts_as_sop1_gives_them() {
        let mut p = Payload::default();
        p.i32(-2);
        p.str("ab");
        p.count(3);
        p.boolean(true);
        p.f32(1.0);
        assert_eq!(
            p.bytes,
            [
                0xfe, 0xff, 0xff, 0xff, 2, 0, 0, 0, b'a', b'b', 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0,
                0x80, 0x3f
            ]
        );
        let mut p = Payload::default();
        put_option_floats(&mut p, &OptionValue::Points(vec![[1., 2.], [3., 4.]]));
        assert_eq!(p.bytes.len(), 8 + 4 * 8);
        let mut p = Payload::default();
        put_option_floats(&mut p, &OptionValue::Int(3));
        assert_eq!(p.bytes, [0; 8]);
    }
}
