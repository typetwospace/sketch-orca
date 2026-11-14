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
// Carries OrcaSlicer code: the parity oracle's src/libslic3r/OracleDump.hpp lines 1-59 and
// OracleDump.cpp lines 1-60, 62-613, 773-785, 797-815, 905-910, 923-934, 995-1017 and 1019-1353
// (sketch-orca oracle patches 0005, 0006 and 0007, on OrcaSlicer v2.4.2), with Print.cpp lines
// 5147-5152 (`PrintInstance::shift_without_plate_offset`) of OrcaSlicer v2.4.2, translated on
// 2026-11-14 (orca-o3-dump-1). Copyright the sketch-orca authors, and for Print.cpp the OrcaSlicer
// authors, the BambuStudio authors (Bambu Lab), the PrusaSlicer authors (Prusa Research) and the
// Slic3r authors (Alessandro Ranellucci), under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).

//! The oracle's dump writer for the object and front-config boundaries (`OracleDump.cpp`,
//! `parity/SOP1.md`): with `SKETCH_ORCA_ORACLE_DUMP` naming a folder, [`dump_object_step`] writes
//! `object<i>-<step>-<n>.sop1` after each print object step and [`dump_front_config`] writes
//! `front-config-<n>.sop1` once the print has its configs, each record as the oracle writes it.
//! Unset, nothing is written and nothing else changes.
//!
//! - [`Payload`] builds a record's payload: little-endian values, floats as their bits, a text as
//!   its u32 length and bytes, a count as a u64. [`Dump`] is one SOP1 file built in memory: the
//!   magic, the version, HEAD and RUN, the records, END with the count of records before it.
//! - The geometry and extrusion writers (`put_*`) and [`config_record`] are shared with the print
//!   records (orca-o3-dump-2) and with orca-cli's front-objects dump, whose `oracle.rs` still
//!   holds its own copies of [`Payload`], the `put_` helpers and [`config_record`]
//!   (orca-o3-process-1 makes it use these).
//! - An entity's kind is the C++'s `typeid`, here the enum's variant. A path inside a loop or a
//!   multi path is an `ExtrusionPath` (kind 1), a sloped loop's starts and ends are sloped paths
//!   (kind 3), and a path's `can_reverse` is the virtual one: false for an oriented path. The
//!   C++'s kind 0 (an entity of another class, written by its type's name) has no Rust
//!   counterpart: every entity is one of the eight variants.
//! - Rust's data model holds indices where the C++ holds pointers: a layer region's region, a
//!   print instance's model instance, a volume region's volume, the shared object. Each is written
//!   as the C++ writes the pointer's index, -1 where the C++ finds none (an index past its list).
//!   A layer's object (`Layer::m_object`, through which a layer region's region is found) is
//!   [`layer_object`]: for a print object that shares another's layers, that other object.
//! - RUN reports the knobs as Rust reads the environment (UTF-8; the oracle's `getenv` reads the
//!   ANSI code page, `parity/HAZARDS.md`), the arena's concurrency, TBB's thread limit as
//!   [`GlobalControl`] sets it (the default concurrency when none does) and the CRT's FMA3 state
//!   (-1 off Windows x64). [`RunState`] holds those values, so that the replay
//!   ([`crate::replay`]) writes a dump again under the RUN record it recorded.
//! - FPRT's `has_wipe_tower`, `has_skirt` and `has_infinite_skirt` are Print.cpp's (575-586 and
//!   3290-3302, orca-o3-print-1's lines in orca-print): the caller passes their answers in
//!   [`PrintAnswers`]. `has_brim` is [`crate::print::has_brim`].
//! - The knob's folder test is Boost's `is_directory` by `status`; here `std::fs::metadata`,
//!   which follows links as `status` does.
//!
//! Translated here though another crate owns it: [`GlobalControl`] and
//! [`global_control_active_value`] (TBB's `global_control` for `max_allowed_parallelism`, which
//! belongs in orca-tbb) and [`shift_without_plate_offset`] (Print.cpp's
//! `PrintInstance::shift_without_plate_offset`, which belongs in orca-print).
//!
//! Left out: line 61, `OracleDumpAccess::wipe_tower_data`, and the print records (lines 614-772,
//! 786-796, 911-922 and 1355-1487: `put_tool_change`, `write_print_dump`, `print_step_name`,
//! `dump_print_step`, `put_conflict_side`, `put_change_stats`, `print_extra_record`,
//! `tool_extra_record`, `put_print_step_records`), orca-o3-dump-2's; and the front-objects dump
//! (lines 816-903 and 935-993), orca-cli's. `OracleDumpAccess`'s other accessors (lines 55-60)
//! are not needed: Rust's `Print` fields they read are public.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use orca_config::config::Config;
use orca_config::{ConfigBase, ConfigError, ConfigOption, OptionValue};
use orca_eigen::Transform3d;
use orca_geom::arc_fitter::PathFittingData;
use orca_geom::bounding_box::BoundingBox;
use orca_geom::expolygon::{ExPolygon, ExPolygons};
use orca_geom::point::{Point, Points, scaled};
use orca_geom::polygon::{Polygon, Polygons};
use orca_geom::polyline::Polylines;
use orca_model::model::{Model, ModelObject};
use orca_model::object_id::ObjectID;
use orca_slice::slicing::SlicingParameters;

use crate::collection::ExtrusionEntityCollection;
use crate::entity::{ExtrusionEntity, RuntimeError};
use crate::extrusion_loop::ExtrusionLoop;
use crate::fs_path;
use crate::layer::{Layer, SupportInnerType, SupportLayer};
use crate::path::{ExtrusionPath, ExtrusionPathSloped};
use crate::print::{Print, has_brim};
use crate::print_base::{PrintState, State, StepType, WarningLevel};
use crate::print_object::{
    FuzzySkinParentType, POS_COUNT, PrintInstance, PrintObject, PrintObjectRegions,
};
use crate::surface::Surfaces;

/// `SOP1_VERSION`.
pub const SOP1_VERSION: u32 = 1;

/// The knob that names the dump folder.
pub const DUMP_KNOB: &str = "SKETCH_ORCA_ORACLE_DUMP";

/// The knobs RUN reports, in its order.
pub const RUN_KNOBS: [&str; 4] = [
    "SKETCH_ORCA_ORACLE_TIME",
    "SKETCH_ORCA_ORACLE_SEED",
    "SKETCH_ORCA_ORACLE_THREADS",
    "SKETCH_ORCA_ORACLE_FMA3",
];

/// `class Payload`: a record's payload, built in memory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Payload {
    bytes: Vec<u8>,
}

impl Payload {
    /// An empty payload.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `u8(v)`.
    pub fn u8(&mut self, v: u8) {
        self.bytes.push(v);
    }

    /// `u16(v)`.
    pub fn u16(&mut self, v: u16) {
        self.le(u64::from(v), 2);
    }

    /// `u32(v)`.
    pub fn u32(&mut self, v: u32) {
        self.le(u64::from(v), 4);
    }

    /// `u64(v)`.
    pub fn u64(&mut self, v: u64) {
        self.le(v, 8);
    }

    /// `i32(v)`: its bits as a u32.
    pub fn i32(&mut self, v: i32) {
        self.u32(v as u32);
    }

    /// `i64(v)`: its bits as a u64.
    pub fn i64(&mut self, v: i64) {
        self.u64(v as u64);
    }

    /// `boolean(v)`: 1 or 0.
    pub fn boolean(&mut self, v: bool) {
        self.u8(u8::from(v));
    }

    /// `f32(v)`: its bits.
    pub fn f32(&mut self, v: f32) {
        self.u32(v.to_bits());
    }

    /// `f64(v)`: its bits.
    pub fn f64(&mut self, v: f64) {
        self.u64(v.to_bits());
    }

    /// `str(s)`: the length as a u32 (`uint32_t(s.size())`), then the bytes.
    pub fn str(&mut self, s: &str) {
        self.str_bytes(s.as_bytes());
    }

    /// `str(s)` of a text held as bytes.
    pub fn str_bytes(&mut self, s: &[u8]) {
        self.u32(s.len() as u32);
        self.bytes.extend_from_slice(s);
    }

    /// `count(n)`: as a u64.
    pub fn count(&mut self, n: usize) {
        self.u64(n as u64);
    }

    /// `bytes()`.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The payload's bytes, taken.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// `le(v, n)`: the n low bytes of v, lowest first.
    fn le(&mut self, v: u64, n: usize) {
        for i in 0..n {
            self.bytes.push((v >> (8 * i)) as u8);
        }
    }
}

// ---- The knob ----

/// `struct DumpKnob`: the dump knob as read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DumpKnob {
    /// `set`: the knob is set.
    pub set: bool,
    /// `valid`: it names an existing folder.
    pub valid: bool,
    /// `folder`: the folder it names.
    pub folder: String,
    /// `problem`: why it is refused, or "".
    pub problem: String,
}

/// `read_dump_knob()`, of the knob's text (`std::getenv("SKETCH_ORCA_ORACLE_DUMP")`).
#[must_use]
pub fn read_dump_knob(text: Option<&str>) -> DumpKnob {
    let mut knob = DumpKnob::default();
    let Some(text) = text else {
        return knob;
    };
    knob.set = true;
    if text.is_empty() {
        knob.problem = "SKETCH_ORCA_ORACLE_DUMP must name an existing folder; it is empty".into();
        return knob;
    }
    knob.folder = text.to_string();
    if !std::fs::metadata(&knob.folder).is_ok_and(|m| m.is_dir()) {
        knob.problem =
            format!("SKETCH_ORCA_ORACLE_DUMP must name an existing folder; {text} is not one");
        return knob;
    }
    knob.valid = true;
    knob
}

/// `dump_knob()`: the knob as first read.
pub fn dump_knob() -> &'static DumpKnob {
    static KNOB: OnceLock<DumpKnob> = OnceLock::new();
    KNOB.get_or_init(|| {
        let text = std::env::var_os(DUMP_KNOB).map(|v| v.to_string_lossy().into_owned());
        read_dump_knob(text.as_deref())
    })
}

/// `boundary_counts()` behind `dump_mutex()`: how many dumps of each file name were made. A dump
/// holds the lock from its start to its write, so dumps are written one at a time, whole.
static BOUNDARY_COUNTS: Mutex<BTreeMap<String, u32>> = Mutex::new(BTreeMap::new());

/// `std::lock_guard<std::mutex> lock(dump_mutex())`, with the boundary counts it guards.
pub fn lock_boundary_counts() -> MutexGuard<'static, BTreeMap<String, u32>> {
    BOUNDARY_COUNTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// `dump_enabled()`: the knob names an existing folder.
#[must_use]
pub fn dump_enabled() -> bool {
    dump_knob().valid
}

/// `dump_knob_problem()`: why the knob is refused, or "".
#[must_use]
pub fn dump_knob_problem() -> String {
    dump_knob().problem.clone()
}

// ---- Geometry and extrusions ----

/// `put_point`: x, then y.
pub fn put_point(p: &mut Payload, pt: &Point) {
    p.i64(pt.x);
    p.i64(pt.y);
}

/// `put_points`.
pub fn put_points(p: &mut Payload, pts: &Points) {
    p.count(pts.len());
    for pt in pts {
        put_point(p, pt);
    }
}

/// `put_polygon`.
pub fn put_polygon(p: &mut Payload, polygon: &Polygon) {
    put_points(p, &polygon.points);
}

/// `put_polygons`.
pub fn put_polygons(p: &mut Payload, polygons: &Polygons) {
    p.count(polygons.len());
    for polygon in polygons {
        put_polygon(p, polygon);
    }
}

/// `put_expolygon`: the contour, then the holes.
pub fn put_expolygon(p: &mut Payload, expolygon: &ExPolygon) {
    put_polygon(p, &expolygon.contour);
    put_polygons(p, &expolygon.holes);
}

/// `put_expolygons`.
pub fn put_expolygons(p: &mut Payload, expolygons: &ExPolygons) {
    p.count(expolygons.len());
    for expolygon in expolygons {
        put_expolygon(p, expolygon);
    }
}

/// `put_fitting`: each fitted span, its arc in full.
pub fn put_fitting(p: &mut Payload, fitting: &[PathFittingData]) {
    p.count(fitting.len());
    for f in fitting {
        p.u64(f.start_point_index as u64);
        p.u64(f.end_point_index as u64);
        p.u8(f.path_type as u8);
        let arc = &f.arc_data;
        p.boolean(arc.is_arc);
        put_point(p, &arc.center);
        p.f64(arc.radius);
        p.f64(arc.length);
        p.f64(arc.angle_radians);
        p.f64(arc.polar_start_theta);
        p.f64(arc.polar_end_theta);
        put_point(p, &arc.start_point);
        put_point(p, &arc.end_point);
        p.u8(arc.direction as u8);
    }
}

/// `put_polylines`: each polyline's points and its fitting.
pub fn put_polylines(p: &mut Payload, polylines: &Polylines) {
    p.count(polylines.len());
    for polyline in polylines {
        put_points(p, &polyline.points);
        put_fitting(p, &polyline.fitting_result);
    }
}

/// `put_surfaces`.
pub fn put_surfaces(p: &mut Payload, surfaces: &Surfaces) {
    p.count(surfaces.len());
    for s in surfaces {
        p.u32(s.surface_type as u32);
        put_expolygon(p, &s.expolygon);
        p.f64(s.thickness);
        p.u16(s.thickness_layers);
        p.f64(s.bridge_angle);
        p.u16(s.extra_perimeters);
    }
}

/// `ekUnknown`: an entity of another class (none in Rust).
pub const EK_UNKNOWN: u8 = 0;
/// `ekPath`.
pub const EK_PATH: u8 = 1;
/// `ekPathContoured`.
pub const EK_PATH_CONTOURED: u8 = 2;
/// `ekPathSloped`.
pub const EK_PATH_SLOPED: u8 = 3;
/// `ekPathOriented`.
pub const EK_PATH_ORIENTED: u8 = 4;
/// `ekMultiPath`.
pub const EK_MULTI_PATH: u8 = 5;
/// `ekLoop`.
pub const EK_LOOP: u8 = 6;
/// `ekLoopSloped`.
pub const EK_LOOP_SLOPED: u8 = 7;
/// `ekCollection`.
pub const EK_COLLECTION: u8 = 8;

/// `put_path_fields`: `can_reverse` is the path's virtual `can_reverse()`, which the caller
/// knows from the path's class (false for an oriented path).
pub fn put_path_fields(p: &mut Payload, path: &ExtrusionPath, can_reverse: bool) {
    p.u8(path.role() as u8);
    p.i32(path.inset_idx);
    p.count(path.polyline.points.len());
    for pt in &path.polyline.points {
        p.i64(pt.x);
        p.i64(pt.y);
        p.i64(pt.z);
    }
    put_fitting(p, &path.polyline.fitting_result);
    p.f64(path.overhang_degree);
    p.i32(path.curve_degree);
    p.f64(path.mm3_per_mm);
    p.f32(path.width);
    p.f32(path.height);
    p.f64(path.smooth_speed);
    p.boolean(path.z_contoured);
    p.boolean(path.is_force_no_extrusion());
    p.boolean(can_reverse);
}

/// `put_slope`.
pub fn put_slope(p: &mut Payload, path: &ExtrusionPathSloped) {
    p.f64(path.slope_begin.z_ratio);
    p.f64(path.slope_begin.e_ratio);
    p.f64(path.slope_end.z_ratio);
    p.f64(path.slope_end.e_ratio);
}

/// `put_loop_fields`: each of the loop's paths is an `ExtrusionPath`.
pub fn put_loop_fields(p: &mut Payload, l: &ExtrusionLoop) {
    p.i32(l.inset_idx);
    p.u8(l.loop_role().0);
    p.count(l.paths.len());
    for path in &l.paths {
        put_path_entity(p, path);
    }
}

/// `put_entity` of an `ExtrusionPath` (`typeid(ExtrusionPath)`).
pub fn put_path_entity(p: &mut Payload, path: &ExtrusionPath) {
    p.u8(EK_PATH);
    put_path_fields(p, path, path.can_reverse());
}

/// `put_entity` of an `ExtrusionPathSloped`.
pub fn put_sloped_entity(p: &mut Payload, path: &ExtrusionPathSloped) {
    p.u8(EK_PATH_SLOPED);
    put_path_fields(p, &path.path, path.path.can_reverse());
    put_slope(p, path);
}

/// `put_entity` of an `ExtrusionEntityCollection`.
pub fn put_collection_entity(p: &mut Payload, collection: &ExtrusionEntityCollection) {
    p.u8(EK_COLLECTION);
    p.i32(collection.inset_idx);
    p.boolean(collection.no_sort);
    p.boolean(collection.can_reverse());
    p.count(collection.entities.len());
    for child in &collection.entities {
        put_entity(p, child);
    }
}

/// `put_entity`: the kind, then the class's fields.
pub fn put_entity(p: &mut Payload, entity: &ExtrusionEntity) {
    match entity {
        ExtrusionEntity::Path(path) => put_path_entity(p, path),
        ExtrusionEntity::PathContoured(path) => {
            p.u8(EK_PATH_CONTOURED);
            put_path_fields(p, &path.path, entity.can_reverse());
            p.count(path.z_diffs.len());
            for &z in &path.z_diffs {
                p.f64(z);
            }
        }
        ExtrusionEntity::PathSloped(path) => put_sloped_entity(p, path),
        ExtrusionEntity::PathOriented(path) => {
            p.u8(EK_PATH_ORIENTED);
            put_path_fields(p, &path.path, path.can_reverse());
        }
        ExtrusionEntity::MultiPath(multi) => {
            p.u8(EK_MULTI_PATH);
            p.i32(multi.inset_idx);
            p.boolean(multi.can_reverse());
            p.count(multi.paths.len());
            for path in &multi.paths {
                put_path_entity(p, path);
            }
        }
        ExtrusionEntity::Loop(l) => {
            p.u8(EK_LOOP);
            put_loop_fields(p, l);
        }
        ExtrusionEntity::LoopSloped(l) => {
            p.u8(EK_LOOP_SLOPED);
            put_loop_fields(p, &l.base);
            p.count(l.starts.len());
            for path in &l.starts {
                put_sloped_entity(p, path);
            }
            p.count(l.ends.len());
            for path in &l.ends {
                put_sloped_entity(p, path);
            }
        }
        ExtrusionEntity::Collection(collection) => put_collection_entity(p, collection),
    }
}

/// `put_matrix`: column by column, as Eigen stores it.
pub fn put_matrix(p: &mut Payload, t: &Transform3d) {
    for c in 0..4 {
        for r in 0..4 {
            p.f64(t.m.at(r, c));
        }
    }
}

// ---- Configs ----

/// `put_option_floats`: the numbers of a float, percent or float-or-percent, their lists, a
/// point, a list of points or a 3D point, coordinate by coordinate; none for any other option.
pub fn put_option_floats(p: &mut Payload, opt: &dyn ConfigOption) {
    let values: Vec<f64> = match opt.to_value() {
        OptionValue::Float(v) | OptionValue::Percent(v) => vec![v],
        OptionValue::FloatOrPercent { value, .. } => vec![value],
        OptionValue::Floats { values, .. } | OptionValue::Percents { values, .. } => values,
        OptionValue::FloatsOrPercents { values, .. } => values.iter().map(|v| v.0).collect(),
        OptionValue::Point(v) => v.to_vec(),
        OptionValue::Points(v) => v.iter().flatten().copied().collect(),
        OptionValue::Point3(v) => v.to_vec(),
        _ => Vec::new(),
    };
    p.count(values.len());
    for x in values {
        p.f64(x);
    }
}

/// `config_record(scope, object, part, config)`: CONF, where the config is from, then every
/// option by key (sorted): its key, its type, its text and its numbers.
///
/// # Errors
///
/// The `ConfigurationError` of an option that does not serialize (a nil in a vector that takes
/// none), which the C++ throws.
pub fn config_record(
    scope: &str,
    object: i64,
    part: i64,
    config: &dyn ConfigBase,
) -> Result<Payload, ConfigError> {
    let mut p = Payload::new();
    p.str(scope);
    p.i64(object);
    p.i64(part);
    let mut keys = config.keys();
    orca_stl::sort(&mut keys);
    p.count(keys.len());
    for key in &keys {
        p.str(key);
        let Some(opt) = config.option(key) else {
            p.u32(0);
            p.str("");
            p.count(0);
            continue;
        };
        p.u32(opt.opt_type().code());
        p.str(&opt.serialize()?);
        put_option_floats(&mut p, opt);
    }
    Ok(p)
}

// ---- The container ----

/// TBB's `global_control` value for `max_allowed_parallelism`, 0 when no control is active.
/// Belongs in orca-tbb.
static MAX_ALLOWED_PARALLELISM: AtomicUsize = AtomicUsize::new(0);

/// `tbb::global_control(max_allowed_parallelism, value)`: while it lives, TBB's thread limit is
/// `value` (the oracle's main holds one of 1 under the thread knob). Belongs in orca-tbb.
#[derive(Debug)]
pub struct GlobalControl {
    previous: usize,
}

impl GlobalControl {
    /// `global_control(max_allowed_parallelism, value)`.
    #[must_use]
    pub fn max_allowed_parallelism(value: usize) -> Self {
        Self {
            previous: MAX_ALLOWED_PARALLELISM.swap(value, Ordering::SeqCst),
        }
    }
}

impl Drop for GlobalControl {
    fn drop(&mut self) {
        MAX_ALLOWED_PARALLELISM.store(self.previous, Ordering::SeqCst);
    }
}

/// `tbb::global_control::active_value(max_allowed_parallelism)`: the active control's value, or
/// the default concurrency when none is active. Belongs in orca-tbb.
#[must_use]
pub fn global_control_active_value() -> usize {
    match MAX_ALLOWED_PARALLELISM.load(Ordering::SeqCst) {
        0 => orca_tbb::default_concurrency(),
        v => v,
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

/// One knob as RUN reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunKnob {
    /// The knob's name.
    pub name: String,
    /// Whether it is set.
    pub set: bool,
    /// Its text, "" when it is not set.
    pub value: String,
}

/// What RUN reports (`run_record()`): the knobs, the arena's concurrency, TBB's thread limit and
/// the CRT's FMA3 state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunState {
    /// The knobs, in [`RUN_KNOBS`]' order.
    pub knobs: Vec<RunKnob>,
    /// `tbb::this_task_arena::max_concurrency()`.
    pub max_concurrency: i32,
    /// `tbb::global_control::active_value(max_allowed_parallelism)`.
    pub max_allowed_parallelism: u64,
    /// `_get_FMA3_enable()`, -1 off Windows x64.
    pub fma3_enable: i32,
}

impl RunState {
    /// The values at this boundary, as `run_record()` reads them.
    #[must_use]
    pub fn current() -> Self {
        let knobs = RUN_KNOBS
            .iter()
            .map(|name| {
                let value = std::env::var_os(name).map(|v| v.to_string_lossy().into_owned());
                RunKnob {
                    name: (*name).to_string(),
                    set: value.is_some(),
                    value: value.unwrap_or_default(),
                }
            })
            .collect();
        Self {
            knobs,
            max_concurrency: orca_tbb::max_concurrency() as i32,
            max_allowed_parallelism: global_control_active_value() as u64,
            fma3_enable: fma3_enable(),
        }
    }

    /// `run_record()`'s payload.
    #[must_use]
    pub fn record(&self) -> Payload {
        let mut p = Payload::new();
        p.count(self.knobs.len());
        for knob in &self.knobs {
            p.str(&knob.name);
            p.boolean(knob.set);
            p.str(&knob.value);
        }
        p.i32(self.max_concurrency);
        p.u64(self.max_allowed_parallelism);
        p.i32(self.fma3_enable);
        p
    }
}

/// The file a dump is written to: `object<i>-<name>` for an object, else `<source>-<name>`, then
/// the boundary's count and `.sop1`.
#[must_use]
pub fn dump_file_name(source: &str, name: &str, object: i64, occurrence: u32) -> String {
    format!("{}-{occurrence}.sop1", dump_base_name(source, name, object))
}

/// The file name before its count, which the boundary counts are kept by.
fn dump_base_name(source: &str, name: &str, object: i64) -> String {
    if source == "object" {
        format!("object{object}-{name}")
    } else {
        format!("{source}-{name}")
    }
}

/// `class Dump`: one SOP1 file, built whole in memory.
#[derive(Clone, Debug)]
pub struct Dump {
    file: String,
    occurrence: u32,
    bytes: Vec<u8>,
    records: u64,
}

impl Dump {
    /// `Dump(source, name, object)`: the next count of the file's name, HEAD, and RUN of this
    /// boundary.
    #[must_use]
    pub fn new(counts: &mut BTreeMap<String, u32>, source: &str, name: &str, object: i64) -> Self {
        let count = counts
            .entry(dump_base_name(source, name, object))
            .or_insert(0);
        *count = count.wrapping_add(1);
        let occurrence = *count;
        Self::with_run(source, name, object, occurrence, &RunState::current())
    }

    /// A dump of the given count and RUN values: the replay's, which writes a dump again under
    /// the HEAD and RUN it recorded.
    #[must_use]
    pub fn with_run(
        source: &str,
        name: &str,
        object: i64,
        occurrence: u32,
        run: &RunState,
    ) -> Self {
        let mut dump = Self {
            file: dump_file_name(source, name, object, occurrence),
            occurrence,
            bytes: b"SOP1".to_vec(),
            records: 0,
        };
        let mut version = Payload::new();
        version.u32(SOP1_VERSION);
        dump.bytes.extend_from_slice(version.bytes());
        let mut head = Payload::new();
        head.str(source);
        head.str(name);
        head.i64(object);
        head.u32(occurrence);
        dump.record(b"HEAD", &head);
        dump.record(b"RUN ", &run.record());
        dump
    }

    /// `record(kind, payload)`: the kind, the payload's length, the payload.
    pub fn record(&mut self, kind: &[u8; 4], payload: &Payload) {
        self.bytes.extend_from_slice(kind);
        let mut length = Payload::new();
        length.u64(payload.bytes().len() as u64);
        self.bytes.extend_from_slice(length.bytes());
        self.bytes.extend_from_slice(payload.bytes());
        self.records += 1;
    }

    /// The file's name (`m_file`).
    #[must_use]
    pub fn file(&self) -> &str {
        &self.file
    }

    /// The boundary's count (`m_occurrence`).
    #[must_use]
    pub fn occurrence(&self) -> u32 {
        self.occurrence
    }

    /// The END record (the number of records before it), then the file's name and bytes.
    #[must_use]
    pub fn finish(mut self) -> (String, Vec<u8>) {
        let mut end = Payload::new();
        end.u64(self.records);
        self.record(b"END ", &end);
        (self.file, self.bytes)
    }

    /// `write()`: [`Dump::finish`], then the file in the dump folder.
    ///
    /// # Errors
    ///
    /// The C++'s `RuntimeError` when the file cannot be written.
    pub fn write(self, folder: &str) -> Result<(), RuntimeError> {
        let (file, bytes) = self.finish();
        std::fs::write(fs_path::append(folder, &file), &bytes).map_err(|_| {
            RuntimeError(format!(
                "sketch-orca-oracle: could not write the dump {file} in SKETCH_ORCA_ORACLE_DUMP"
            ))
        })
    }
}

/// What a front-config dump can fail on: an option that does not serialize, or the write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DumpError {
    /// `ConfigurationError` from `serialize()`.
    Config(ConfigError),
    /// The write's `RuntimeError`.
    Write(RuntimeError),
}

impl core::fmt::Display for DumpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Config(e) => write!(f, "{}", e.0),
            Self::Write(e) => write!(f, "{}", e.0),
        }
    }
}

impl std::error::Error for DumpError {}

// ---- Print state ----

/// `object_index(print, object)`: the object's index in the print, -1 when it is not the
/// print's.
#[must_use]
pub fn object_index(print: &Print, object: &PrintObject) -> i64 {
    print
        .objects
        .iter()
        .position(|o| core::ptr::eq(o, object))
        .map_or(-1, |i| i as i64)
}

/// `object_index(print, id)`: the index of the print's object of that id, -1 for none.
#[must_use]
pub fn object_index_by_id(print: &Print, id: ObjectID) -> i64 {
    print
        .objects
        .iter()
        .position(|o| o.object_id() == id)
        .map_or(-1, |i| i as i64)
}

/// `put_object_ids`: each id as its object's index.
pub fn put_object_ids(p: &mut Payload, print: &Print, ids: &[ObjectID]) {
    p.count(ids.len());
    for &id in ids {
        p.i64(object_index_by_id(print, id));
    }
}

/// `put_entity_map`: the map's collections by object index, then by id (a stable sort of the
/// map's order), so that the order does not depend on the ids' values.
pub fn put_entity_map(
    p: &mut Payload,
    print: &Print,
    map: &BTreeMap<ObjectID, ExtrusionEntityCollection>,
) {
    let mut items: Vec<(i64, &ExtrusionEntityCollection)> = map
        .iter()
        .map(|(id, c)| (object_index_by_id(print, *id), c))
        .collect();
    orca_stl::stable_sort_by(&mut items, |a, b| a.0 < b.0);
    p.count(items.len());
    for (index, collection) in items {
        p.i64(index);
        put_collection_entity(p, collection);
    }
}

/// `object_record(object, index)`: OBJ.
#[must_use]
pub fn object_record(model: &Model, object: &PrintObject, index: i64) -> Payload {
    let mut p = Payload::new();
    p.i64(index);
    p.str(
        model
            .objects
            .get(object.base.base.model_object)
            .map_or("", |m| m.name.as_str()),
    );
    p.i64(object.size().x());
    p.i64(object.size().y());
    p.i64(object.size().z());
    put_point(&mut p, object.center_offset());
    put_matrix(&mut p, object.trafo());
    p.f64(object.max_z());
    p.count(object.instances().len());
    for instance in object.instances() {
        put_point(&mut p, &instance.shift);
        p.u64(instance.id as u64);
    }
    p.count(object.layers().len());
    p.count(object.support_layers().len());
    p
}

/// `layer->object()` for the layers `object` holds: the print object whose layers they are. A
/// print object that shares another's layers (`m_shared_object`) holds that object's layers once
/// `copy_layers_from_shared_object` has run, which copies the `Layer` pointers, whose `m_object`
/// stays the other object; so their regions are that object's. Before the copy such an object
/// holds no layers.
#[must_use]
pub fn layer_object<'a>(print: &'a Print, object: &'a PrintObject) -> &'a PrintObject {
    object
        .get_shared_object()
        .and_then(|s| print.objects.get(s))
        .unwrap_or(object)
}

/// `layer_record(layer, position)`: LAYR. A region is written by its print region's id, found
/// through the layer's object (`object`, from [`layer_object`]).
#[must_use]
pub fn layer_record(object: &PrintObject, layer: &Layer, position: usize) -> Payload {
    let mut p = Payload::new();
    p.u64(position as u64);
    p.u64(layer.id() as u64);
    p.f64(layer.slice_z);
    p.f64(layer.print_z);
    p.f64(layer.height);
    p.boolean(layer.slicing_errors);
    put_expolygons(&mut p, &layer.lslices);
    put_expolygons(&mut p, &layer.lslices_extrudable);
    put_expolygons(&mut p, &layer.loverhangs);
    put_expolygons(&mut p, &layer.sharp_tails);
    put_expolygons(&mut p, &layer.cantilevers);
    p.count(layer.sharp_tails_height.len());
    for &h in &layer.sharp_tails_height {
        p.f32(h);
    }
    p.count(layer.regions().len());
    for region in layer.regions() {
        p.i32(region.region(object).print_region_id());
        put_surfaces(&mut p, &region.slices.surfaces);
        put_expolygons(&mut p, &region.raw_slices);
        put_surfaces(&mut p, &region.fill_surfaces.surfaces);
        put_expolygons(&mut p, &region.fill_expolygons);
        put_expolygons(&mut p, &region.fill_no_overlap_expolygons);
        put_polylines(&mut p, &region.unsupported_bridge_edges);
        put_collection_entity(&mut p, &region.thin_fills);
        put_collection_entity(&mut p, &region.perimeters);
        put_collection_entity(&mut p, &region.fills);
    }
    p
}

/// `support_layer_record(layer, position)`: SUPL.
#[must_use]
pub fn support_layer_record(layer: &SupportLayer, position: usize) -> Payload {
    let mut p = Payload::new();
    p.u64(position as u64);
    p.u64(layer.layer.id() as u64);
    p.f64(layer.layer.slice_z);
    p.f64(layer.layer.print_z);
    p.f64(layer.layer.height);
    p.u32(match layer.support_type {
        SupportInnerType::Normal => 0,
        SupportInnerType::Tree => 1,
    });
    p.u64(layer.interface_id() as u64);
    put_expolygons(&mut p, &layer.layer.lslices);
    put_expolygons(&mut p, &layer.support_islands);
    put_expolygons(&mut p, &layer.base_areas);
    put_collection_entity(&mut p, &layer.support_fills);
    p
}

/// The records of an object dump after HEAD and RUN (`write_object_dump` before its `write()`):
/// OBJ, a LAYR per layer, a SUPL per support layer, then OSTA, OWRN and a LEXT per layer.
pub fn put_object_dump_records(dump: &mut Dump, print: &Print, object: &PrintObject, index: i64) {
    dump.record(b"OBJ ", &object_record(print.model(), object, index));
    let layers_of = layer_object(print, object);
    for (position, layer) in object.layers().iter().enumerate() {
        dump.record(b"LAYR", &layer_record(layers_of, layer, position));
    }
    for (position, layer) in object.support_layers().iter().enumerate() {
        dump.record(b"SUPL", &support_layer_record(layer, position));
    }
    put_object_step_records(dump, print, object, index);
}

/// `write_object_dump(object, index, step)`.
///
/// # Errors
///
/// The write's `RuntimeError`.
pub fn write_object_dump(
    counts: &mut BTreeMap<String, u32>,
    print: &Print,
    object: &PrintObject,
    index: i64,
    step: &str,
) -> Result<(), RuntimeError> {
    let mut dump = Dump::new(counts, "object", step, index);
    put_object_dump_records(&mut dump, print, object, index);
    dump.write(&dump_knob().folder)
}

/// `object_step_name(step)`: the step's name in Print.hpp, none outside the steps.
#[must_use]
pub fn object_step_name(step: i32) -> Option<&'static str> {
    const NAMES: [&str; POS_COUNT] = [
        "posSlice",
        "posPerimeters",
        "posEstimateCurledExtrusions",
        "posPrepareInfill",
        "posInfill",
        "posIroning",
        "posContouring",
        "posSupportMaterial",
        "posSimplifyPath",
        "posSimplifySupportPath",
        "posDetectOverhangsForLift",
        "posSimplifyWall",
        "posSimplifyInfill",
    ];
    usize::try_from(step)
        .ok()
        .and_then(|s| NAMES.get(s))
        .copied()
}

/// `dump_object_step(object, step)`: with the knob set, `object<i>-<step>-<n>.sop1` of the
/// print's object `object`. The C++ takes the object and finds its print through its back
/// pointer; here the print and the object's index are given, and an index past the objects (the
/// C++'s object without a print) writes nothing.
///
/// # Errors
///
/// The write's `RuntimeError`.
pub fn dump_object_step(print: &Print, object: usize, step: i32) -> Result<(), RuntimeError> {
    if !dump_enabled() {
        return Ok(());
    }
    let name = object_step_name(step);
    let (Some(object), Some(name)) = (print.objects.get(object), name) else {
        return Ok(());
    };
    let mut counts = lock_boundary_counts();
    write_object_dump(
        &mut counts,
        print,
        object,
        object_index(print, object),
        name,
    )
}

/// The records of a front-config dump after HEAD and RUN (`dump_front_config` before its
/// `write()`): CONF job, print-full and print, each object's CONF object and, when it has
/// regions, a CONF region per printing region, then [`put_front_config_records`].
///
/// # Errors
///
/// The `ConfigurationError` of an option that does not serialize.
pub fn put_front_config_dump_records(
    dump: &mut Dump,
    print: &Print,
    config: &Config,
    answers: &PrintAnswers,
) -> Result<(), ConfigError> {
    dump.record(b"CONF", &config_record("job", -1, -1, config)?);
    dump.record(
        b"CONF",
        &config_record("print-full", -1, -1, print.full_print_config())?,
    );
    dump.record(b"CONF", &config_record("print", -1, -1, print.config())?);
    for (i, object) in print.objects().iter().enumerate() {
        dump.record(
            b"CONF",
            &config_record("object", i as i64, -1, object.config())?,
        );
        if object.shared_regions().is_none() {
            continue;
        }
        for r in 0..object.num_printing_regions() {
            dump.record(
                b"CONF",
                &config_record(
                    "region",
                    i as i64,
                    r as i64,
                    object.printing_region(r).config(),
                )?,
            );
        }
    }
    put_front_config_records(dump, print, answers)
}

/// `dump_front_config(print, config)`: with the knob set, `front-config-<n>.sop1`; `config` is
/// the front end's, `answers` what Print.cpp's `has_wipe_tower`, `has_skirt` and
/// `has_infinite_skirt` give for this print.
///
/// # Errors
///
/// An option that does not serialize, or the write.
pub fn dump_front_config(
    print: &Print,
    config: &Config,
    answers: &PrintAnswers,
) -> Result<(), DumpError> {
    if !dump_enabled() {
        return Ok(());
    }
    let mut counts = lock_boundary_counts();
    let mut dump = Dump::new(&mut counts, "front", "config", -1);
    put_front_config_dump_records(&mut dump, print, config, answers).map_err(DumpError::Config)?;
    dump.write(&dump_knob().folder).map_err(DumpError::Write)
}

// ==== sketch-orca oracle patch 0007 (2026-11-13): the step boundaries in full ====

// ---- Shared by the three parts ----

/// `index_in(list, value)` where the value is an index into the list: the index, or -1 when it
/// is past the list (the C++'s pointer that is not in it).
#[must_use]
pub fn index_in(len: usize, at: usize) -> i64 {
    if at < len { at as i64 } else { -1 }
}

/// `volume_index(object, id)`: the index of the model object's volume of that id, -1 for none
/// or no model object.
#[must_use]
pub fn volume_index(object: Option<&ModelObject>, id: ObjectID) -> i64 {
    let Some(object) = object else {
        return -1;
    };
    object
        .volumes
        .iter()
        .position(|v| v.id() == id)
        .map_or(-1, |i| i as i64)
}

/// `put_bbox`: defined, min, max.
pub fn put_bbox(p: &mut Payload, b: &BoundingBox) {
    p.boolean(b.defined);
    put_point(p, &b.min);
    put_point(p, &b.max);
}

/// `PrintStateBase::State` as its value.
#[must_use]
pub fn state_value(state: State) -> u8 {
    match state {
        State::Invalid => 0,
        State::Started => 1,
        State::Done => 2,
    }
}

/// `PrintStateBase::WarningLevel` as its value.
#[must_use]
pub fn warning_level_value(level: WarningLevel) -> u8 {
    match level {
        WarningLevel::NonCritical => 0,
        WarningLevel::Critical => 1,
    }
}

/// `put_step_states<Base, Step>(p, base, count)`: for each step, its state and warnings (level,
/// current, message, message id); the time stamps left out.
pub fn put_step_states<S: StepType, const COUNT: usize>(
    p: &mut Payload,
    state: &PrintState<S, COUNT>,
) {
    p.count(COUNT);
    for s in 0..COUNT {
        let state = state.state_with_warnings(S::from_index(s));
        p.u8(state_value(state.state));
        p.count(state.warnings.len());
        for w in &state.warnings {
            p.u8(warning_level_value(w.level));
            p.boolean(w.current);
            p.str(&w.message);
            p.i32(w.message_id);
        }
    }
}

/// `put_slicing_parameters`: every field in its order.
pub fn put_slicing_parameters(p: &mut Payload, s: &SlicingParameters) {
    p.boolean(s.valid);
    p.u64(s.base_raft_layers as u64);
    p.u64(s.interface_raft_layers as u64);
    p.f64(s.base_raft_layer_height);
    p.f64(s.interface_raft_layer_height);
    p.f64(s.contact_raft_layer_height);
    p.f64(s.layer_height);
    p.f64(s.min_layer_height);
    p.f64(s.max_layer_height);
    p.f64(s.max_suport_layer_height);
    p.f64(s.first_print_layer_height);
    p.f64(s.first_object_layer_height);
    p.boolean(s.first_object_layer_bridging);
    p.boolean(s.zero_gap_interface_top);
    p.boolean(s.zero_gap_interface_bottom);
    p.boolean(s.zero_gap_interface_raft);
    p.f64(s.gap_raft_object);
    p.f64(s.gap_object_support);
    p.f64(s.gap_support_object);
    p.f64(s.raft_base_top_z);
    p.f64(s.raft_interface_top_z);
    p.f64(s.raft_contact_top_z);
    p.f64(s.object_print_z_min);
    p.f64(s.object_print_z_max);
    p.f64(s.object_print_z_uncompensated_max);
    p.f64(s.object_shrinkage_compensation_z);
}

/// `PrintInstance::shift_without_plate_offset()` (Print.cpp 5147-5152): the shift less the
/// plate's origin, scaled. Belongs in orca-print.
#[must_use]
pub fn shift_without_plate_offset(print: &Print, instance: &PrintInstance) -> Point {
    let plate_offset = print.get_plate_origin();
    instance.shift - Point::new(scaled(plate_offset.x()), scaled(plate_offset.y()))
}

// ---- The object records (object<i>-<step>-<n>.sop1) ----

/// `object_state_record(object, index)`: OSTA.
#[must_use]
pub fn object_state_record(print: &Print, object: &PrintObject, index: i64) -> Payload {
    let mut p = Payload::new();
    let model = print.model().objects.get(object.base.base.model_object);
    p.i64(index);
    p.i64(object.get_shared_object().map_or(-1, |s| {
        print
            .objects
            .get(s)
            .map_or(-1, |shared| object_index(print, shared))
    }));
    put_slicing_parameters(&mut p, object.slicing_parameters());
    p.count(object.instances().len());
    for instance in object.instances() {
        p.u64(instance.unique_id as u64);
        p.i64(model.map_or(-1, |m| index_in(m.instances.len(), instance.model_instance)));
        put_point(&mut p, &shift_without_plate_offset(print, instance));
    }
    p.count(object.object_first_layer_wall_extruders.len());
    for &e in &object.object_first_layer_wall_extruders {
        p.i32(e);
    }
    put_bbox(&mut p, &object.first_layer_object_brim_bounding_box);
    put_collection_entity(&mut p, object.object_skirt());
    p.count(object.first_layer_obj_slice().len());
    for v in object.first_layer_obj_slice() {
        p.i64(volume_index(model, v.volume_id));
        p.count(v.slices.len());
        for slices in &v.slices {
            put_expolygons(&mut p, slices);
        }
    }
    p.count(object.first_layer_obj_groups().len());
    for g in object.first_layer_obj_groups() {
        p.i32(g.group_id);
        p.count(g.volume_ids.len());
        for &id in &g.volume_ids {
            p.i64(volume_index(model, id));
        }
        put_expolygons(&mut p, &g.slices);
    }
    p
}

/// `object_warnings_record(object, index)`: OWRN.
#[must_use]
pub fn object_warnings_record(object: &PrintObject, index: i64) -> Payload {
    let mut p = Payload::new();
    p.i64(index);
    put_step_states(&mut p, &object.base.state);
    p
}

/// `layer_extra_record(layers, layer, position)`: LEXT.
#[must_use]
pub fn layer_extra_record(layers: &[Layer], layer: &Layer, position: usize) -> Payload {
    let mut p = Payload::new();
    p.u64(position as u64);
    p.i64(layer.lower_layer.map_or(-1, |l| index_in(layers.len(), l)));
    p.i64(layer.upper_layer.map_or(-1, |l| index_in(layers.len(), l)));
    p.count(layer.lslices_bboxes.len());
    for b in &layer.lslices_bboxes {
        put_bbox(&mut p, b);
    }
    put_bbox(&mut p, &layer.loverhangs_bbox);
    p.count(layer.curled_lines.len());
    for line in &layer.curled_lines {
        put_point(&mut p, &line.line.a);
        put_point(&mut p, &line.line.b);
        p.f32(line.curled_height);
    }
    p
}

/// `put_object_step_records(dump, object, index)`: OSTA, OWRN, a LEXT per layer.
pub fn put_object_step_records(dump: &mut Dump, print: &Print, object: &PrintObject, index: i64) {
    dump.record(b"OSTA", &object_state_record(print, object, index));
    dump.record(b"OWRN", &object_warnings_record(object, index));
    let layers = object.layers();
    for (position, layer) in layers.iter().enumerate() {
        dump.record(b"LEXT", &layer_extra_record(layers, layer, position));
    }
}

// ---- The front-config records (front-config-<n>.sop1) ----

/// What FPRT reports that Print.cpp computes and orca-print will translate: the caller's answers
/// of `has_wipe_tower()`, `has_skirt()` and `has_infinite_skirt()` for the print.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrintAnswers {
    /// `Print::has_wipe_tower()`.
    pub has_wipe_tower: bool,
    /// `Print::has_skirt()`.
    pub has_skirt: bool,
    /// `Print::has_infinite_skirt()`.
    pub has_infinite_skirt: bool,
}

/// `front_print_record(print)`: FPRT.
#[must_use]
pub fn front_print_record(print: &Print, answers: &PrintAnswers) -> Payload {
    let mut p = Payload::new();
    p.i32(print.get_plate_index());
    p.boolean(print.get_no_check_flag());
    p.boolean(print.is_bbl_printer());
    let origin = print.get_plate_origin();
    p.f64(origin.x());
    p.f64(origin.y());
    p.f64(origin.z());
    let maps = print.get_filament_maps();
    p.count(maps.len());
    for m in maps {
        p.i32(m);
    }
    p.count(print.num_print_regions());
    for r in 0..print.num_print_regions() {
        p.i32(print.get_print_region(r).print_region_id());
    }
    p.boolean(answers.has_wipe_tower);
    p.boolean(answers.has_skirt);
    p.boolean(answers.has_infinite_skirt);
    p.boolean(has_brim(print));
    p
}

/// `put_layer_ranges(p, model, regions)`.
pub fn put_layer_ranges(
    p: &mut Payload,
    model: Option<&ModelObject>,
    regions: &PrintObjectRegions,
) {
    let region_id = |r: Option<usize>| -> i32 {
        r.and_then(|r| regions.all_regions.get(r))
            .map_or(-1, |r| r.print_object_region_id())
    };
    p.count(regions.layer_ranges.len());
    for range in &regions.layer_ranges {
        p.f64(range.layer_height_range.0);
        p.f64(range.layer_height_range.1);
        p.boolean(range.config.is_some());
        p.count(range.volumes.len());
        for v in &range.volumes {
            p.i64(volume_index(model, v.volume_id));
            for k in 0..3 {
                p.f32(v.bbox.min[k]);
            }
            for k in 0..3 {
                p.f32(v.bbox.max[k]);
            }
        }
        p.count(range.volume_regions.len());
        for v in &range.volume_regions {
            p.i64(v.model_volume.map_or(-1, |i| {
                model
                    .and_then(|m| m.volumes.get(i))
                    .map_or(-1, |volume| volume_index(model, volume.id()))
            }));
            p.i32(v.parent);
            p.i32(region_id(v.region));
        }
        p.count(range.painted_regions.len());
        for v in &range.painted_regions {
            p.u32(v.extruder_id);
            p.i32(v.parent);
            p.i32(region_id(v.region));
        }
        p.count(range.fuzzy_skin_painted_regions.len());
        for v in &range.fuzzy_skin_painted_regions {
            p.u8(match v.parent_type {
                FuzzySkinParentType::VolumeRegion => 0,
                FuzzySkinParentType::PaintedRegion => 1,
            });
            p.i32(v.parent);
            p.i32(region_id(v.region));
        }
    }
}

/// `front_object_record(print, object, index)`: FOBJ.
#[must_use]
pub fn front_object_record(print: &Print, object: &PrintObject, index: i64) -> Payload {
    let mut p = Payload::new();
    let objects = &print.model().objects;
    let model = objects.get(object.base.base.model_object);
    p.i64(index);
    p.i64(index_in(objects.len(), object.base.base.model_object));
    p.i64(object.size().x());
    p.i64(object.size().y());
    p.i64(object.size().z());
    put_point(&mut p, object.center_offset());
    put_matrix(&mut p, object.trafo());
    p.f64(object.max_z());
    p.count(object.instances().len());
    for instance in object.instances() {
        put_point(&mut p, &instance.shift);
        p.u64(instance.id as u64);
        p.u64(instance.unique_id as u64);
        p.i64(model.map_or(-1, |m| index_in(m.instances.len(), instance.model_instance)));
    }
    put_slicing_parameters(&mut p, object.slicing_parameters());
    p.count(model.map_or(0, |m| m.volumes.len()));
    if let Some(m) = model {
        for volume in &m.volumes {
            p.i32(volume.volume_type().value());
            put_matrix(&mut p, volume.get_matrix());
            p.u64(volume.mesh().its.vertices.len() as u64);
            p.u64(volume.mesh().its.indices.len() as u64);
        }
    }
    let regions = object.shared_regions();
    p.boolean(regions.is_some());
    if let Some(regions) = regions {
        put_matrix(&mut p, &regions.trafo_bboxes);
        p.count(regions.all_regions.len());
        for r in &regions.all_regions {
            p.i32(r.print_region_id());
            p.i32(r.print_object_region_id());
        }
        put_layer_ranges(&mut p, model, regions);
    }
    p
}

/// `put_front_config_records(dump, print)`: CONF default-object, CONF default-region, FPRT, a
/// FOBJ per print object.
///
/// # Errors
///
/// The `ConfigurationError` of an option that does not serialize.
pub fn put_front_config_records(
    dump: &mut Dump,
    print: &Print,
    answers: &PrintAnswers,
) -> Result<(), ConfigError> {
    dump.record(
        b"CONF",
        &config_record("default-object", -1, -1, print.default_object_config())?,
    );
    dump.record(
        b"CONF",
        &config_record("default-region", -1, -1, print.default_region_config())?,
    );
    dump.record(b"FPRT", &front_print_record(print, answers));
    for (i, object) in print.objects().iter().enumerate() {
        dump.record(b"FOBJ", &front_object_record(print, object, i as i64));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::Slope;
    use crate::role::{ExtrusionLoopRole, ExtrusionRole};
    use orca_geom::point3::Point3;

    #[test]
    fn payloads_are_little_endian_with_texts_and_counts_as_sop1_gives_them() {
        let mut p = Payload::new();
        p.u8(1);
        p.u16(0x0203);
        p.i32(-2);
        p.i64(-1);
        p.boolean(true);
        p.f32(1.0);
        p.f64(-0.0);
        p.str("ab");
        p.count(3);
        let mut want = vec![1u8, 3, 2, 0xfe, 0xff, 0xff, 0xff];
        want.extend_from_slice(&[0xff; 8]);
        want.push(1);
        want.extend_from_slice(&1.0f32.to_bits().to_le_bytes());
        want.extend_from_slice(&(-0.0f64).to_bits().to_le_bytes());
        want.extend_from_slice(&[2, 0, 0, 0, b'a', b'b']);
        want.extend_from_slice(&[3, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(p.bytes(), &want[..]);
    }

    #[test]
    fn the_dump_knob_names_an_existing_folder() {
        assert_eq!(read_dump_knob(None), DumpKnob::default());
        let empty = read_dump_knob(Some(""));
        assert!(empty.set && !empty.valid);
        assert_eq!(
            empty.problem,
            "SKETCH_ORCA_ORACLE_DUMP must name an existing folder; it is empty"
        );
        let missing = read_dump_knob(Some("Z:/no/such/folder/at/all"));
        assert_eq!(
            missing.problem,
            "SKETCH_ORCA_ORACLE_DUMP must name an existing folder; Z:/no/such/folder/at/all is not one"
        );
        let here = read_dump_knob(Some(env!("CARGO_MANIFEST_DIR")));
        assert!(here.valid && here.problem.is_empty());
    }

    #[test]
    fn a_dump_is_head_and_run_then_its_records_then_end_with_their_count() {
        let mut counts = BTreeMap::new();
        let run = RunState {
            knobs: vec![RunKnob {
                name: "K".into(),
                set: true,
                value: "1".into(),
            }],
            max_concurrency: 1,
            max_allowed_parallelism: 1,
            fma3_enable: -1,
        };
        let first = Dump::new(&mut counts, "object", "posSlice", 2);
        assert_eq!(first.file(), "object2-posSlice-1.sop1");
        let second = Dump::new(&mut counts, "object", "posSlice", 2);
        assert_eq!(second.file(), "object2-posSlice-2.sop1");
        assert_eq!(
            Dump::new(&mut counts, "front", "config", -1).file(),
            "front-config-1.sop1"
        );
        let mut dump = Dump::with_run("front", "config", -1, 7, &run);
        let mut x = Payload::new();
        x.u8(9);
        dump.record(b"FPRT", &x);
        let (file, bytes) = dump.finish();
        assert_eq!(file, "front-config-7.sop1");
        assert_eq!(&bytes[..8], b"SOP1\x01\x00\x00\x00");
        let end = &bytes[bytes.len() - 20..];
        assert_eq!(&end[..4], b"END ");
        assert_eq!(&end[4..12], &8u64.to_le_bytes());
        assert_eq!(&end[12..], &3u64.to_le_bytes());
    }

    #[test]
    fn step_names_cover_the_object_steps_only() {
        assert_eq!(object_step_name(0), Some("posSlice"));
        assert_eq!(object_step_name(12), Some("posSimplifyInfill"));
        assert_eq!(object_step_name(13), None);
        assert_eq!(object_step_name(-1), None);
    }

    #[test]
    fn entities_are_written_by_their_class_with_the_virtual_can_reverse() {
        let mut path = ExtrusionPath::new(ExtrusionRole::Perimeter, 0.5, 0.4, 0.2);
        path.polyline.points = vec![Point3::new(1, 2, 3), Point3::new(4, 5, 6)];
        let oriented =
            crate::path::ExtrusionPathOriented::new(ExtrusionRole::Perimeter, 0.5, 0.4, 0.2);
        let mut p = Payload::new();
        put_entity(&mut p, &ExtrusionEntity::Path(path.clone()));
        assert_eq!(p.bytes()[0], EK_PATH);
        assert_eq!(*p.bytes().last().unwrap(), 1);
        let mut q = Payload::new();
        put_entity(&mut q, &ExtrusionEntity::PathOriented(oriented));
        assert_eq!(q.bytes()[0], EK_PATH_ORIENTED);
        assert_eq!(*q.bytes().last().unwrap(), 0);
        let l = ExtrusionLoop::from_path(&path, ExtrusionLoopRole::DEFAULT);
        let mut r = Payload::new();
        put_entity(&mut r, &ExtrusionEntity::Loop(l));
        assert_eq!(r.bytes()[0], EK_LOOP);
        // inset, loop role, one path written as kind 1.
        assert_eq!(r.bytes()[6..14], 1u64.to_le_bytes());
        assert_eq!(r.bytes()[14], EK_PATH);
        let sloped = ExtrusionPathSloped::new(
            &path,
            Slope {
                z_ratio: 0.5,
                e_ratio: 0.25,
            },
            Slope {
                z_ratio: 1.0,
                e_ratio: 1.0,
            },
        );
        let mut s = Payload::new();
        put_entity(&mut s, &ExtrusionEntity::PathSloped(sloped));
        assert_eq!(s.bytes()[0], EK_PATH_SLOPED);
        let tail = &s.bytes()[s.bytes().len() - 32..];
        assert_eq!(&tail[..8], &0.5f64.to_bits().to_le_bytes());
    }

    #[test]
    fn a_config_record_lists_its_options_by_key_with_their_numbers() {
        let mut c = Config::default();
        c.create("layer_height").unwrap();
        c.create("wall_loops").unwrap();
        c.create("printable_area").unwrap();
        let p = config_record("job", -1, -1, &c).unwrap();
        let b = p.bytes();
        assert_eq!(&b[..7], b"\x03\x00\x00\x00job");
        // Three options, the first by key "layer_height".
        assert_eq!(&b[23..31], &3u64.to_le_bytes());
        assert_eq!(&b[31..35], &12u32.to_le_bytes());
        assert_eq!(&b[35..47], b"layer_height");
    }

    #[test]
    fn global_control_sets_the_thread_limit_while_it_lives() {
        {
            let _g = GlobalControl::max_allowed_parallelism(1);
            assert_eq!(global_control_active_value(), 1);
        }
        assert_eq!(
            global_control_active_value(),
            orca_tbb::default_concurrency()
        );
    }

    #[test]
    fn index_in_is_minus_one_past_the_list() {
        assert_eq!(index_in(3, 2), 2);
        assert_eq!(index_in(3, 3), -1);
        assert_eq!(index_in(0, usize::MAX), -1);
    }
}
