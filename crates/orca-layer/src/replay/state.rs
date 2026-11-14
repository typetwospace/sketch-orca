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
// Carries no OrcaSlicer code: it reads the front-objects, front-config and object dumps the parity
// oracle's OracleDump.cpp writes (lines 164-613, 773-784, 816-1014 and 1047-1353, sketch-orca
// oracle patches 0005, 0006 and 0007) back into Rust's data model. Written on 2026-11-14
// (orca-o3-dump-1).

//! The dumps read into the data model, so that a later crate can start a step from the state the
//! oracle had before it and compare what it makes with the dump after it:
//!
//! - [`read_front_objects`]: front-objects into a [`Model`] (each object's name, printable flag,
//!   layer height profile, instances, volumes with their meshes and configs, height ranges and
//!   painting), with the run's config, the plates' custom G-code (as its record) and the brim
//!   ears ([`BrimEar`], kept beside the model: orca-model's `BrimPoint` has no constructor yet).
//! - [`read_front_config`]: front-config into a [`Print`] over that model: its configs, its plate,
//!   its objects (each as posSlice finds it, with its config and shared regions) and its print
//!   regions, with the job's config and FPRT's answers ([`PrintAnswers`]).
//!   [`write_front_config`] writes it again.
//! - [`read_object_dump`]: an object dump into the print's object (its layers, support layers,
//!   OSTA's state, its step states and warnings, and LEXT's per-layer values);
//!   [`write_object_dump`] writes it again.
//!
//! What a dump does not hold is rebuilt as the C++ has it where it can be: a region's config hash
//! (`config.hash()`), a volume region's box (the index of its volume's extents) and the step
//! states (by `set_started`, `active_step_add_warning`, `set_done` and `invalidate`, which leave
//! the warnings as recorded). A collection with `no_sort` keeps `is_reverse` true: its
//! `can_reverse()` is false whatever `is_reverse` is, so the dump does not hold it. A layer
//! region's region is the first of its layer's object's regions with the recorded print region id
//! (the layer's object is the one whose layers the object shares, when OSTA names one). A
//! volume region's `prev_same_region` is none, as `Print::apply` leaves it. A layer range's config
//! is a copy of the model object's height range of the same bounds, an empty config when it has
//! none.
//!
//! A value the data model cannot hold is refused with a [`ReplayError`] that names it (a path
//! of an unknown role, an oriented path that may be reversed, a collection with `no_sort` that may
//! be reversed, step warnings in an order no sequence of state changes leaves). Painted facets,
//! which domain A never has (the painted-facets family), stop with `NotPorted`: orca-model's
//! `TriangleBitStreamMapping` has no constructor.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use orca_config::config::{Config, create_default};
use orca_config::{ConfigBase, OptionValue, StaticPrintConfig};
use orca_eigen::{Mat4, Transform3d, Vec3crd, Vec3d, Vec3f, Vec3i32};
use orca_geom::arc_fitter::{EMovePathType, PathFittingData};
use orca_geom::bounding_box::BoundingBox;
use orca_geom::circle::{ArcDirection, ArcSegment};
use orca_geom::expolygon::{ExPolygon, ExPolygons};
use orca_geom::point::{Point, Points};
use orca_geom::point3::Point3;
use orca_geom::polygon::Polygon;
use orca_geom::polyline::{Polyline, Polylines};
use orca_index::aligned_box::AlignedBox;
use orca_index::stop::not_ported;
use orca_mesh::admesh::IndexedTriangleSet;
use orca_mesh::triangle_mesh::TriangleMesh;
use orca_model::config::ModelConfig;
use orca_model::facets::{FacetsAnnotation, TriangleSplittingData};
use orca_model::model::{Model, ModelVolumeType};
use orca_model::object_id::ObjectID;
use orca_model::transformation::Transformation;
use orca_slice::slicing::SlicingParameters;

use super::records::{Record, decode_record};
use super::sop1::{RawRecord, Reader, ReplayError, Result, read_container};
use crate::collection::ExtrusionEntityCollection;
use crate::dump::{
    Dump, PrintAnswers, RunKnob, RunState, put_front_config_dump_records, put_object_dump_records,
};
use crate::entity::ExtrusionEntity;
use crate::extrusion_loop::{ExtrusionLoop, ExtrusionLoopSloped};
use crate::layer::{CurledLine, Layer, LayerRegion, SupportInnerType, SupportLayer};
use crate::multi_path::ExtrusionMultiPath;
use crate::path::{
    ExtrusionPath, ExtrusionPathContoured, ExtrusionPathOriented, ExtrusionPathSloped, Slope,
};
use crate::print::{Print, PrintRegionPtr};
use crate::print_base::{PrintState, StepType, WarningLevel};
use crate::print_object::{
    FuzzySkinPaintedRegion, FuzzySkinParentType, GroupedVolumeSlices, LayerRangeRegions,
    PaintedRegion, PrintInstance, PrintObject, PrintObjectRegions, VolumeExtents, VolumeRegion,
    VolumeSlices,
};
use crate::print_region::PrintRegion;
use crate::role::{ExtrusionLoopRole, ExtrusionRole};
use crate::surface::{Surface, SurfaceType, Surfaces};

/// HEAD: what a dump is of.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Head {
    /// "object", "print" or "front".
    pub source: String,
    /// The step's or the boundary's name.
    pub name: String,
    /// The object's index, -1 for none.
    pub object: i64,
    /// The boundary's count for the file's name.
    pub occurrence: u32,
}

/// HEAD's payload read.
///
/// # Errors
///
/// A payload that is not HEAD's.
pub fn read_head(payload: &[u8]) -> Result<Head> {
    let mut r = Reader::new(payload);
    let head = Head {
        source: r.str()?,
        name: r.str()?,
        object: r.i64()?,
        occurrence: r.u32()?,
    };
    r.finish()?;
    Ok(head)
}

/// RUN's payload read.
///
/// # Errors
///
/// A payload that is not RUN's.
pub fn read_run(payload: &[u8]) -> Result<RunState> {
    let mut r = Reader::new(payload);
    let n = r.count(13)?;
    let mut knobs = Vec::with_capacity(n);
    for _ in 0..n {
        knobs.push(RunKnob {
            name: r.str()?,
            set: r.boolean()?,
            value: r.str()?,
        });
    }
    let run = RunState {
        knobs,
        max_concurrency: r.i32()?,
        max_allowed_parallelism: r.u64()?,
        fma3_enable: r.i32()?,
    };
    r.finish()?;
    Ok(run)
}

/// The records of a dump with HEAD and RUN read; the rest from the third record to END
/// (excluded).
fn open(bytes: &[u8]) -> Result<(Head, RunState, Vec<RawRecord<'_>>)> {
    let records = read_container(bytes)?;
    if records.len() < 3 || &records[0].kind != b"HEAD" || &records[1].kind != b"RUN " {
        return Err(ReplayError::new(
            "a dump that does not start with HEAD and RUN",
        ));
    }
    let head = read_head(records[0].payload).map_err(|e| e.context("HEAD"))?;
    let run = read_run(records[1].payload).map_err(|e| e.context("RUN"))?;
    let body = records[2..records.len() - 1].to_vec();
    Ok((head, run, body))
}

/// A record's payload read by `f` to its end, an error naming the record otherwise.
fn read_record<T>(
    index: usize,
    record: &RawRecord<'_>,
    f: impl FnOnce(&mut Reader<'_>) -> Result<T>,
) -> Result<T> {
    let mut r = Reader::new(record.payload);
    let out = f(&mut r).and_then(|v| r.finish().map(|()| v));
    out.map_err(|e| e.context(&format!("record {} ({})", index + 2, record.kind_name())))
}

fn expect_kind(index: usize, record: &RawRecord<'_>, kind: &[u8; 4]) -> Result<()> {
    if &record.kind == kind {
        Ok(())
    } else {
        Err(ReplayError::new(format!(
            "record {} is {}, where {} was expected",
            index + 2,
            record.kind_name(),
            String::from_utf8_lossy(kind)
        )))
    }
}

// ---- Geometry ----

fn point(r: &mut Reader) -> Result<Point> {
    Ok(Point::new(r.i64()?, r.i64()?))
}

fn points(r: &mut Reader) -> Result<Points> {
    let n = r.count(16)?;
    let mut out = Points::with_capacity(n);
    for _ in 0..n {
        out.push(point(r)?);
    }
    Ok(out)
}

fn polygons(r: &mut Reader) -> Result<Vec<Polygon>> {
    let n = r.count(8)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(Polygon::new(points(r)?));
    }
    Ok(out)
}

fn expolygon(r: &mut Reader) -> Result<ExPolygon> {
    let mut out = ExPolygon::new(Polygon::new(points(r)?));
    out.holes = polygons(r)?;
    Ok(out)
}

fn expolygons(r: &mut Reader) -> Result<ExPolygons> {
    let n = r.count(16)?;
    let mut out = ExPolygons::with_capacity(n);
    for _ in 0..n {
        out.push(expolygon(r)?);
    }
    Ok(out)
}

fn bbox(r: &mut Reader) -> Result<BoundingBox> {
    let mut out = BoundingBox::new();
    out.defined = r.boolean()?;
    out.min = point(r)?;
    out.max = point(r)?;
    Ok(out)
}

fn matrix(r: &mut Reader) -> Result<Transform3d> {
    let mut v = [0.0f64; 16];
    for x in &mut v {
        *x = r.f64()?;
    }
    Ok(Transform3d::from_matrix(Mat4::from_fn(|row, col| {
        v[col * 4 + row]
    })))
}

fn path_type(v: u8) -> Result<EMovePathType> {
    Ok(match v {
        0 => EMovePathType::NoopMove,
        1 => EMovePathType::LinearMove,
        2 => EMovePathType::ArcMoveCw,
        3 => EMovePathType::ArcMoveCcw,
        _ => return Err(ReplayError::new(format!("a path type of {v}"))),
    })
}

fn arc_direction(v: u8) -> Result<ArcDirection> {
    Ok(match v {
        0 => ArcDirection::Unknown,
        1 => ArcDirection::Ccw,
        2 => ArcDirection::Cw,
        _ => return Err(ReplayError::new(format!("an arc direction of {v}"))),
    })
}

/// A fitted span's arc, its fields in the order `put_fitting` writes them.
fn read_arc(r: &mut Reader, arc: &mut ArcSegment) -> Result<()> {
    arc.is_arc = r.boolean()?;
    arc.center = point(r)?;
    arc.radius = r.f64()?;
    arc.length = r.f64()?;
    arc.angle_radians = r.f64()?;
    arc.polar_start_theta = r.f64()?;
    arc.polar_end_theta = r.f64()?;
    arc.start_point = point(r)?;
    arc.end_point = point(r)?;
    arc.direction = arc_direction(r.u8()?)?;
    Ok(())
}

fn fitting(r: &mut Reader) -> Result<Vec<PathFittingData>> {
    let n = r.count(115)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let start = r.u64()? as usize;
        let end = r.u64()? as usize;
        let ty = path_type(r.u8()?)?;
        let mut arc = ArcSegment::default();
        read_arc(r, &mut arc)?;
        out.push(PathFittingData::new(start, end, ty, arc));
    }
    Ok(out)
}

fn polylines(r: &mut Reader) -> Result<Polylines> {
    let n = r.count(16)?;
    let mut out = Polylines::with_capacity(n);
    for _ in 0..n {
        let mut polyline = Polyline::new(points(r)?);
        polyline.fitting_result = fitting(r)?;
        out.push(polyline);
    }
    Ok(out)
}

fn surfaces(r: &mut Reader) -> Result<Surfaces> {
    let n = r.count(40)?;
    let mut out = Surfaces::with_capacity(n);
    for _ in 0..n {
        let ty = r.u32()?;
        let ty = i32::try_from(ty)
            .ok()
            .and_then(SurfaceType::from_i32)
            .ok_or_else(|| ReplayError::new(format!("a surface type of {ty}")))?;
        let mut s = Surface::new(ty);
        s.expolygon = expolygon(r)?;
        s.thickness = r.f64()?;
        s.thickness_layers = r.u16()?;
        s.bridge_angle = r.f64()?;
        s.extra_perimeters = r.u16()?;
        out.push(s);
    }
    Ok(out)
}

// ---- Extrusions ----

/// A path's fields, and the `can_reverse` it was written with.
fn path_fields(r: &mut Reader) -> Result<(ExtrusionPath, bool)> {
    let role = r.u8()?;
    let role = ExtrusionRole::from_u8(role)
        .ok_or_else(|| ReplayError::new(format!("a path of the role {role}")))?;
    let inset_idx = r.i32()?;
    let n = r.count(24)?;
    let mut pts = Vec::with_capacity(n);
    for _ in 0..n {
        pts.push(Point3::new(r.i64()?, r.i64()?, r.i64()?));
    }
    let fitting_result = fitting(r)?;
    let overhang_degree = r.f64()?;
    let curve_degree = r.i32()?;
    let mm3_per_mm = r.f64()?;
    let width = r.f32()?;
    let height = r.f32()?;
    let smooth_speed = r.f64()?;
    let z_contoured = r.boolean()?;
    let no_extrusion = r.boolean()?;
    let can_reverse = r.boolean()?;
    let mut path = ExtrusionPath::new_no_extrusion(role, mm3_per_mm, width, height, no_extrusion);
    path.inset_idx = inset_idx;
    path.polyline.points = pts;
    path.polyline.fitting_result = fitting_result;
    path.overhang_degree = overhang_degree;
    path.curve_degree = curve_degree;
    path.smooth_speed = smooth_speed;
    path.z_contoured = z_contoured;
    if !can_reverse {
        path.set_reverse();
    }
    Ok((path, can_reverse))
}

fn slope(r: &mut Reader) -> Result<Slope> {
    Ok(Slope {
        z_ratio: r.f64()?,
        e_ratio: r.f64()?,
    })
}

fn kind_error(at: usize, kind: u8, wanted: &str) -> ReplayError {
    ReplayError::new(format!(
        "an entity of kind {kind} where {wanted} was expected (at byte {at})"
    ))
}

/// An entity that must be an `ExtrusionPath` (kind 1): a loop's or a multi path's path.
fn path_entity(r: &mut Reader) -> Result<ExtrusionPath> {
    let at = r.position();
    match r.u8()? {
        1 => Ok(path_fields(r)?.0),
        k => Err(kind_error(at, k, "a path (kind 1)")),
    }
}

/// An entity that must be an `ExtrusionPathSloped` (kind 3): a sloped loop's start or end.
fn sloped_entity(r: &mut Reader) -> Result<ExtrusionPathSloped> {
    let at = r.position();
    match r.u8()? {
        3 => {
            let (path, _) = path_fields(r)?;
            let slope_begin = slope(r)?;
            let slope_end = slope(r)?;
            Ok(ExtrusionPathSloped {
                path,
                slope_begin,
                slope_end,
            })
        }
        k => Err(kind_error(at, k, "a sloped path (kind 3)")),
    }
}

fn paths(r: &mut Reader) -> Result<Vec<ExtrusionPath>> {
    let n = r.count(1)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(path_entity(r)?);
    }
    Ok(out)
}

fn sloped_paths(r: &mut Reader) -> Result<Vec<ExtrusionPathSloped>> {
    let n = r.count(1)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(sloped_entity(r)?);
    }
    Ok(out)
}

fn loop_fields(r: &mut Reader) -> Result<ExtrusionLoop> {
    let inset_idx = r.i32()?;
    let role = ExtrusionLoopRole(r.u8()?);
    let mut l = ExtrusionLoop::from_paths(paths(r)?, role);
    l.inset_idx = inset_idx;
    Ok(l)
}

/// A collection's fields after its kind.
fn collection_fields(r: &mut Reader, at: usize) -> Result<ExtrusionEntityCollection> {
    let mut c = ExtrusionEntityCollection::default();
    c.inset_idx = r.i32()?;
    c.no_sort = r.boolean()?;
    let can_reverse = r.boolean()?;
    if c.no_sort && can_reverse {
        return Err(ReplayError::new(format!(
            "a collection with no_sort that may be reversed (at byte {at})"
        )));
    }
    if !c.no_sort && !can_reverse {
        c.set_reverse();
    }
    let n = r.count(1)?;
    c.entities.reserve(n);
    for _ in 0..n {
        c.entities.push(entity(r)?);
    }
    Ok(c)
}

/// An entity that must be a collection (kind 8): a layer region's fills, a skirt.
fn collection_entity(r: &mut Reader) -> Result<ExtrusionEntityCollection> {
    let at = r.position();
    match r.u8()? {
        8 => collection_fields(r, at),
        k => Err(kind_error(at, k, "a collection (kind 8)")),
    }
}

/// An entity of any kind.
fn entity(r: &mut Reader) -> Result<ExtrusionEntity> {
    let at = r.position();
    Ok(match r.u8()? {
        1 => ExtrusionEntity::Path(path_fields(r)?.0),
        2 => {
            let (path, _) = path_fields(r)?;
            let n = r.count(8)?;
            let mut z_diffs = Vec::with_capacity(n);
            for _ in 0..n {
                z_diffs.push(r.f64()?);
            }
            ExtrusionEntity::PathContoured(ExtrusionPathContoured { path, z_diffs })
        }
        3 => {
            let (path, _) = path_fields(r)?;
            let slope_begin = slope(r)?;
            let slope_end = slope(r)?;
            ExtrusionEntity::PathSloped(ExtrusionPathSloped {
                path,
                slope_begin,
                slope_end,
            })
        }
        4 => {
            let (mut path, can_reverse) = path_fields(r)?;
            if can_reverse {
                return Err(ReplayError::new(format!(
                    "an oriented path that may be reversed (at byte {at})"
                )));
            }
            // The member is not written: an oriented path's can_reverse() is false whatever it
            // is. The constructors leave it true.
            path = rebuild_reversible(path);
            ExtrusionEntity::PathOriented(ExtrusionPathOriented { path })
        }
        5 => {
            let mut m = ExtrusionMultiPath::default();
            m.inset_idx = r.i32()?;
            let can_reverse = r.boolean()?;
            m.paths = paths(r)?;
            if !can_reverse {
                m.set_reverse();
            }
            ExtrusionEntity::MultiPath(m)
        }
        6 => ExtrusionEntity::Loop(loop_fields(r)?),
        7 => {
            let base = loop_fields(r)?;
            let starts = sloped_paths(r)?;
            let ends = sloped_paths(r)?;
            ExtrusionEntity::LoopSloped(ExtrusionLoopSloped { base, starts, ends })
        }
        8 => ExtrusionEntity::Collection(collection_fields(r, at)?),
        k => {
            return Err(ReplayError::new(format!(
                "an entity of kind {k}, which Rust's entities cannot hold (at byte {at})"
            )));
        }
    })
}

/// The path with its `can_reverse` member as its constructors leave it (true).
fn rebuild_reversible(path: ExtrusionPath) -> ExtrusionPath {
    let mut out = ExtrusionPath::new_no_extrusion(
        path.role(),
        path.mm3_per_mm,
        path.width,
        path.height,
        path.is_force_no_extrusion(),
    );
    out.inset_idx = path.inset_idx;
    out.polyline = path.polyline;
    out.overhang_degree = path.overhang_degree;
    out.curve_degree = path.curve_degree;
    out.smooth_speed = path.smooth_speed;
    out.z_contoured = path.z_contoured;
    out
}

// ---- Configs ----

/// One option of a CONF record.
#[derive(Clone, Debug, PartialEq)]
struct ConfOption {
    key: String,
    ty: u32,
    text: String,
    floats: Vec<f64>,
}

/// A CONF record.
#[derive(Clone, Debug, PartialEq)]
struct Conf {
    scope: String,
    object: i64,
    part: i64,
    options: Vec<ConfOption>,
}

fn read_conf(r: &mut Reader) -> Result<Conf> {
    let scope = r.str()?;
    let object = r.i64()?;
    let part = r.i64()?;
    let n = r.count(20)?;
    let mut options = Vec::with_capacity(n);
    for _ in 0..n {
        let key = r.str()?;
        let ty = r.u32()?;
        let text = r.str()?;
        let m = r.count(8)?;
        let mut floats = Vec::with_capacity(m);
        for _ in 0..m {
            floats.push(r.f64()?);
        }
        options.push(ConfOption {
            key,
            ty,
            text,
            floats,
        });
    }
    Ok(Conf {
        scope,
        object,
        part,
        options,
    })
}

/// The option's value: `base` (a value of its type) read from the recorded text, then its
/// numbers set to the recorded bits, which the text gives only to six digits.
fn option_value(mut v: OptionValue, o: &ConfOption) -> Result<OptionValue> {
    let fail = |what: &str| ReplayError::new(format!("option {}: {what}", o.key));
    if v.ty().code() != o.ty {
        return Err(fail(&format!(
            "recorded of type {}, Rust's is of type {}",
            o.ty,
            v.ty().code()
        )));
    }
    if !v.deserialize(&o.text).map_err(|e| fail(&e.0))? {
        return Err(fail(&format!("the text {:?} does not read", o.text)));
    }
    let f = &o.floats;
    let wrong = || fail(&format!("{} numbers do not fit its value", f.len()));
    match &mut v {
        OptionValue::Float(x) | OptionValue::Percent(x) => {
            let [a] = f[..] else { return Err(wrong()) };
            *x = a;
        }
        OptionValue::FloatOrPercent { value, .. } => {
            let [a] = f[..] else { return Err(wrong()) };
            *value = a;
        }
        OptionValue::Floats { values, .. } | OptionValue::Percents { values, .. } => {
            if values.len() != f.len() {
                return Err(wrong());
            }
            values.copy_from_slice(f);
        }
        OptionValue::FloatsOrPercents { values, .. } => {
            if values.len() != f.len() {
                return Err(wrong());
            }
            for (v, &a) in values.iter_mut().zip(f) {
                v.0 = a;
            }
        }
        OptionValue::Point(p) => {
            let [a, b] = f[..] else { return Err(wrong()) };
            *p = [a, b];
        }
        OptionValue::Points(ps) => {
            if ps.len() * 2 != f.len() {
                return Err(wrong());
            }
            for (p, ab) in ps.iter_mut().zip(f.chunks(2)) {
                *p = [ab[0], ab[1]];
            }
        }
        OptionValue::Point3(p) => {
            let [a, b, c] = f[..] else {
                return Err(wrong());
            };
            *p = [a, b, c];
        }
        _ => {
            if !f.is_empty() {
                return Err(wrong());
            }
        }
    }
    Ok(v)
}

/// The recorded options set on a config, each made (a dynamic config) or found (a static one).
fn apply_conf(cfg: &mut dyn ConfigBase, conf: &Conf) -> Result<()> {
    for o in &conf.options {
        let opt = cfg.option_mut(&o.key, true).ok_or_else(|| {
            ReplayError::new(format!("option {}: not an option of Rust's config", o.key))
        })?;
        let v = option_value(opt.to_value(), o)?;
        opt.set_value(&v)
            .map_err(|e| ReplayError::new(format!("option {}: {}", o.key, e.0)))?;
    }
    Ok(())
}

fn dynamic_config(conf: &Conf) -> Result<Config> {
    let mut out = Config::default();
    apply_conf(&mut out, conf)?;
    Ok(out)
}

fn static_config<T: StaticPrintConfig + Default>(conf: &Conf) -> Result<T> {
    let mut out = T::default();
    apply_conf(&mut out, conf)?;
    Ok(out)
}

fn model_config(conf: &Conf) -> Result<ModelConfig> {
    let mut out = ModelConfig::new();
    for o in &conf.options {
        let def = orca_config::get(&o.key).ok_or_else(|| {
            ReplayError::new(format!("option {}: not an option Orca defines", o.key))
        })?;
        out.set_key_value(&o.key, option_value(create_default(def), o)?);
    }
    Ok(out)
}

fn scope_error(index: usize, conf: &Conf, wanted: &str) -> ReplayError {
    ReplayError::new(format!(
        "record {}: CONF {} ({}, {}) where CONF {wanted} was expected",
        index + 2,
        conf.scope,
        conf.object,
        conf.part
    ))
}

// ---- front-objects ----

/// A brim ear of front-objects' BRIM (`BrimPoint`: its position and head radius).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrimEar {
    /// `pos`.
    pub pos: Vec3f,
    /// `head_front_radius`.
    pub head_front_radius: f32,
}

/// A front-objects dump read.
#[derive(Debug)]
pub struct FrontObjects {
    /// HEAD.
    pub head: Head,
    /// RUN.
    pub run: RunState,
    /// CONF run: the front end's config at that point.
    pub config: Config,
    /// PCGC, as its record.
    pub plates_custom_gcodes: Record,
    /// The model.
    pub model: Model,
    /// Each object's brim ears.
    pub brim_ears: Vec<Vec<BrimEar>>,
}

fn volume_type(v: i32) -> Result<ModelVolumeType> {
    Ok(match v {
        -1 => ModelVolumeType::Invalid,
        0 => ModelVolumeType::ModelPart,
        1 => ModelVolumeType::NegativeVolume,
        2 => ModelVolumeType::ParameterModifier,
        3 => ModelVolumeType::SupportBlocker,
        4 => ModelVolumeType::SupportEnforcer,
        _ => return Err(ReplayError::new(format!("a volume type of {v}"))),
    })
}

/// MODL into a new object of the model.
fn read_modl(r: &mut Reader, model: &mut Model) -> Result<usize> {
    let index = r.i64()?;
    let i = model.add_empty_object();
    if index != i as i64 {
        return Err(ReplayError::new(format!(
            "MODL of object {index} as object {i}"
        )));
    }
    let object = &mut model.objects[i];
    object.name = r.str()?;
    object.printable = r.boolean()?;
    let n = r.count(8)?;
    let mut profile = Vec::with_capacity(n);
    for _ in 0..n {
        profile.push(r.f64()?);
    }
    object.layer_height_profile = profile;
    let n = r.count(129)?;
    for _ in 0..n {
        let m = matrix(r)?;
        let k = object.add_instance();
        object.instances[k].set_transformation(Transformation::new(&m));
        object.instances[k].printable = r.boolean()?;
    }
    let n = r.count(157)?;
    for _ in 0..n {
        let name = r.str()?;
        let ty = volume_type(r.i32()?)?;
        let m = matrix(r)?;
        let nv = r.count(12)?;
        let mut vertices = Vec::with_capacity(nv);
        for _ in 0..nv {
            vertices.push(Vec3f::new(r.f32()?, r.f32()?, r.f32()?));
        }
        let nt = r.count(12)?;
        let mut indices = Vec::with_capacity(nt);
        for _ in 0..nt {
            indices.push(Vec3i32::new(r.i32()?, r.i32()?, r.i32()?));
        }
        let mesh = TriangleMesh::from_its(IndexedTriangleSet::new(indices, vertices));
        let v = object.add_volume(mesh, ty, false);
        object.volumes[v].name = name;
        object.volumes[v].set_transformation(Transformation::new(&m));
    }
    Ok(i)
}

/// One painting of PNTV into a volume's facets.
fn read_painting(r: &mut Reader, facets: &mut FacetsAnnotation) -> Result<()> {
    if r.count(8)? != 0 {
        not_ported(
            "replay::read_front_objects",
            "painted facets (the painted-facets family, outside domain A): orca-model's \
             TriangleBitStreamMapping has no constructor",
        );
    }
    let bits = usize::try_from(r.u64()?)
        .map_err(|_| ReplayError::new("a bitstream longer than memory"))?;
    let bytes = r.count(1)?;
    if bytes != bits.div_ceil(8) {
        return Err(ReplayError::new(format!(
            "a bitstream of {bits} bits in {bytes} bytes"
        )));
    }
    let packed = r.take(bytes)?;
    let bitstream = (0..bits)
        .map(|i| (packed[i / 8] >> (i % 8)) & 1 == 1)
        .collect();
    let n = r.count(1)?;
    let mut used_states = Vec::with_capacity(n);
    for _ in 0..n {
        used_states.push(r.boolean()?);
    }
    facets.set_data(TriangleSplittingData {
        bitstream,
        used_states,
        ..Default::default()
    });
    Ok(())
}

/// A front-objects dump read into a model.
///
/// # Errors
///
/// A dump that is not front-objects', or a value the model cannot hold.
pub fn read_front_objects(bytes: &[u8]) -> Result<FrontObjects> {
    let (head, run, body) = open(bytes)?;
    let mut it = body.iter().enumerate();
    let mut next = |kind: &[u8; 4]| -> Result<(usize, RawRecord<'_>)> {
        let (i, rec) = it
            .next()
            .ok_or_else(|| ReplayError::new("the dump ends early"))?;
        expect_kind(i, rec, kind)?;
        Ok((i, *rec))
    };
    let (i, rec) = next(b"CONF")?;
    let conf = read_record(i, &rec, read_conf)?;
    if conf.scope != "run" {
        return Err(scope_error(i, &conf, "run"));
    }
    let config = dynamic_config(&conf).map_err(|e| e.context("CONF run"))?;
    let (i, rec) = next(b"PCGC")?;
    let plates_custom_gcodes = decode_record(&rec.kind, rec.payload)
        .map_err(|e| e.context(&format!("record {} (PCGC)", i + 2)))?;
    let mut model = Model::default();
    let mut brim_ears = Vec::new();
    let mut rest = body.iter().enumerate().skip(2);
    while let Some((i, rec)) = rest.next() {
        expect_kind(i, rec, b"MODL")?;
        let o = read_record(i, rec, |r| read_modl(r, &mut model))?;
        let mut take = |kind: &[u8; 4]| -> Result<(usize, RawRecord<'_>)> {
            let (i, rec) = rest
                .next()
                .ok_or_else(|| ReplayError::new("the dump ends early"))?;
            expect_kind(i, rec, kind)?;
            Ok((i, *rec))
        };
        let (i, rec) = take(b"CONF")?;
        let conf = read_record(i, &rec, read_conf)?;
        if conf.scope != "model-object" || conf.object != o as i64 {
            return Err(scope_error(i, &conf, "model-object"));
        }
        model.objects[o].config = model_config(&conf)?;
        for v in 0..model.objects[o].volumes.len() {
            let (i, rec) = take(b"CONF")?;
            let conf = read_record(i, &rec, read_conf)?;
            if conf.scope != "model-volume" || conf.object != o as i64 || conf.part != v as i64 {
                return Err(scope_error(i, &conf, "model-volume"));
            }
            model.objects[o].volumes[v].config = model_config(&conf)?;
        }
        let (i, rec) = take(b"HRNG")?;
        let ranges = read_record(i, &rec, |r| {
            if r.i64()? != o as i64 {
                return Err(ReplayError::new("HRNG of another object"));
            }
            let n = r.count(16)?;
            let mut out = Vec::with_capacity(n);
            for _ in 0..n {
                out.push((r.f64()?, r.f64()?));
            }
            Ok(out)
        })?;
        for (k, range) in ranges.into_iter().enumerate() {
            let (i, rec) = take(b"CONF")?;
            let conf = read_record(i, &rec, read_conf)?;
            if conf.scope != "model-height-range"
                || conf.object != o as i64
                || conf.part != k as i64
            {
                return Err(scope_error(i, &conf, "model-height-range"));
            }
            model.objects[o]
                .layer_config_ranges
                .push((range, model_config(&conf)?));
        }
        let (i, rec) = take(b"BRIM")?;
        brim_ears.push(read_record(i, &rec, |r| {
            if r.i64()? != o as i64 {
                return Err(ReplayError::new("BRIM of another object"));
            }
            let n = r.count(16)?;
            let mut out = Vec::with_capacity(n);
            for _ in 0..n {
                out.push(BrimEar {
                    pos: Vec3f::new(r.f32()?, r.f32()?, r.f32()?),
                    head_front_radius: r.f32()?,
                });
            }
            Ok(out)
        })?);
        for v in 0..model.objects[o].volumes.len() {
            let (i, rec) = take(b"PNTV")?;
            let volume = &mut model.objects[o].volumes[v];
            read_record(i, &rec, |r| {
                if r.i64()? != o as i64 || r.i64()? != v as i64 {
                    return Err(ReplayError::new("PNTV of another volume"));
                }
                read_painting(r, &mut volume.supported_facets)?;
                read_painting(r, &mut volume.seam_facets)?;
                read_painting(r, &mut volume.mmu_segmentation_facets)?;
                read_painting(r, &mut volume.fuzzy_skin_facets)
            })?;
        }
    }
    Ok(FrontObjects {
        head,
        run,
        config,
        plates_custom_gcodes,
        model,
        brim_ears,
    })
}

// ---- front-config ----

/// A front-config dump read.
#[derive(Debug)]
pub struct FrontConfig {
    /// HEAD.
    pub head: Head,
    /// RUN.
    pub run: RunState,
    /// CONF job: the front end's config.
    pub job: Config,
    /// The print, over the model it was read with.
    pub print: Print,
    /// FPRT's `has_wipe_tower`, `has_skirt` and `has_infinite_skirt`.
    pub answers: PrintAnswers,
}

/// FOBJ's and OSTA's slicing parameters.
fn slicing_parameters(r: &mut Reader) -> Result<SlicingParameters> {
    let mut s = SlicingParameters::default();
    read_slicing_parameters(r, &mut s)?;
    Ok(s)
}

/// The slicing parameters' fields, in the order `put_slicing_parameters` writes them.
fn read_slicing_parameters(r: &mut Reader, s: &mut SlicingParameters) -> Result<()> {
    s.valid = r.boolean()?;
    s.base_raft_layers = r.u64()? as usize;
    s.interface_raft_layers = r.u64()? as usize;
    s.base_raft_layer_height = r.f64()?;
    s.interface_raft_layer_height = r.f64()?;
    s.contact_raft_layer_height = r.f64()?;
    s.layer_height = r.f64()?;
    s.min_layer_height = r.f64()?;
    s.max_layer_height = r.f64()?;
    s.max_suport_layer_height = r.f64()?;
    s.first_print_layer_height = r.f64()?;
    s.first_object_layer_height = r.f64()?;
    s.first_object_layer_bridging = r.boolean()?;
    s.zero_gap_interface_top = r.boolean()?;
    s.zero_gap_interface_bottom = r.boolean()?;
    s.zero_gap_interface_raft = r.boolean()?;
    s.gap_raft_object = r.f64()?;
    s.gap_object_support = r.f64()?;
    s.gap_support_object = r.f64()?;
    s.raft_base_top_z = r.f64()?;
    s.raft_interface_top_z = r.f64()?;
    s.raft_contact_top_z = r.f64()?;
    s.object_print_z_min = r.f64()?;
    s.object_print_z_max = r.f64()?;
    s.object_print_z_uncompensated_max = r.f64()?;
    s.object_shrinkage_compensation_z = r.f64()?;
    Ok(())
}

/// An index of -1 or more as an index, -1 (the C++'s pointer not found) as `usize::MAX`.
fn index_or_max(v: i64) -> Result<usize> {
    if v == -1 {
        Ok(usize::MAX)
    } else {
        usize::try_from(v).map_err(|_| ReplayError::new(format!("an index of {v}")))
    }
}

/// A volume's index as its id, -1 as no id.
fn volume_id(ids: &[ObjectID], v: i64) -> Result<ObjectID> {
    if v == -1 {
        return Ok(ObjectID::default());
    }
    usize::try_from(v)
        .ok()
        .and_then(|v| ids.get(v).copied())
        .ok_or_else(|| ReplayError::new(format!("a volume index of {v} past the volumes")))
}

/// A region's print object region id as its index in the regions (FOBJ's list of print region
/// and print object region ids), -1 as none.
fn region_index(regions: &[(i32, i32)], id: i32) -> Result<Option<usize>> {
    if id == -1 {
        return Ok(None);
    }
    regions
        .iter()
        .position(|r| r.1 == id)
        .map(Some)
        .ok_or_else(|| ReplayError::new(format!("a region of print object region id {id}")))
}

/// What FOBJ holds of an object's regions.
struct FobjRegions {
    trafo_bboxes: Transform3d,
    all_regions: Vec<(i32, i32)>,
    layer_ranges: Vec<LayerRangeRegions>,
}

fn read_layer_ranges(
    r: &mut Reader,
    ids: &[ObjectID],
    region_ids: &[(i32, i32)],
    model_ranges: &[((f64, f64), ModelConfig)],
) -> Result<Vec<LayerRangeRegions>> {
    let regions = region_ids;
    let n = r.count(17)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let layer_height_range = (r.f64()?, r.f64()?);
        let config = if r.boolean()? {
            let mut config = Config::default();
            if let Some((_, c)) = model_ranges.iter().find(|(b, _)| *b == layer_height_range) {
                for (k, v) in c.iter() {
                    config.insert(k, v.clone());
                }
            }
            Some(config)
        } else {
            None
        };
        let m = r.count(32)?;
        let mut volumes = Vec::with_capacity(m);
        for _ in 0..m {
            let volume_id = volume_id(ids, r.i64()?)?;
            let min = [r.f32()?, r.f32()?, r.f32()?];
            let max = [r.f32()?, r.f32()?, r.f32()?];
            volumes.push(VolumeExtents {
                volume_id,
                bbox: AlignedBox::new(
                    Vec3f::new(min[0], min[1], min[2]),
                    Vec3f::new(max[0], max[1], max[2]),
                ),
            });
        }
        let m = r.count(16)?;
        let mut volume_regions = Vec::with_capacity(m);
        for _ in 0..m {
            let model_volume = index_or_max(r.i64()?)?;
            let model_volume = (model_volume != usize::MAX).then_some(model_volume);
            let parent = r.i32()?;
            let region = region_index(regions, r.i32()?)?;
            let bbox = model_volume.and_then(|v| {
                let id = ids.get(v).copied()?;
                volumes
                    .iter()
                    .position(|e: &VolumeExtents| e.volume_id == id)
            });
            volume_regions.push(VolumeRegion {
                model_volume,
                parent,
                region,
                bbox,
                prev_same_region: None,
            });
        }
        let m = r.count(12)?;
        let mut painted_regions = Vec::with_capacity(m);
        for _ in 0..m {
            let extruder_id = r.u32()?;
            let parent = r.i32()?;
            let region = region_index(regions, r.i32()?)?;
            painted_regions.push(PaintedRegion {
                extruder_id,
                parent,
                region,
            });
        }
        let m = r.count(9)?;
        let mut fuzzy_skin_painted_regions = Vec::with_capacity(m);
        for _ in 0..m {
            let parent_type = match r.u8()? {
                0 => FuzzySkinParentType::VolumeRegion,
                1 => FuzzySkinParentType::PaintedRegion,
                t => return Err(ReplayError::new(format!("a fuzzy skin parent type of {t}"))),
            };
            let parent = r.i32()?;
            let region = region_index(regions, r.i32()?)?;
            fuzzy_skin_painted_regions.push(FuzzySkinPaintedRegion {
                parent_type,
                parent,
                region,
            });
        }
        out.push(LayerRangeRegions {
            layer_height_range,
            config,
            volumes,
            volume_regions,
            painted_regions,
            fuzzy_skin_painted_regions,
        });
    }
    Ok(out)
}

/// FOBJ into a print object (its config and regions' configs set by the caller).
fn read_fobj(
    r: &mut Reader,
    model: &Model,
    index: usize,
) -> Result<(PrintObject, Option<FobjRegions>)> {
    if r.i64()? != index as i64 {
        return Err(ReplayError::new(format!(
            "FOBJ of another object than {index}"
        )));
    }
    let model_object = index_or_max(r.i64()?)?;
    let size = Vec3crd::new(r.i64()?, r.i64()?, r.i64()?);
    let center_offset = point(r)?;
    let trafo = matrix(r)?;
    let mut object = PrintObject::with_members(model_object, trafo);
    object.size = size;
    object.center_offset = center_offset;
    object.max_z = r.f64()?;
    let n = r.count(40)?;
    for _ in 0..n {
        let shift = point(r)?;
        let id = r.u64()? as usize;
        let unique_id = r.u64()? as usize;
        let mut instance = PrintInstance::new(index_or_max(r.i64()?)?, shift);
        instance.id = id;
        instance.unique_id = unique_id;
        object.instances.push(instance);
    }
    object.slicing_params = slicing_parameters(r)?;
    let mo = model.objects.get(model_object);
    let n = r.count(148)?;
    if n != mo.map_or(0, |m| m.volumes.len()) {
        return Err(ReplayError::new(format!(
            "FOBJ lists {n} model volumes, the model object has {}",
            mo.map_or(0, |m| m.volumes.len())
        )));
    }
    for k in 0..n {
        let ty = r.i32()?;
        let m = matrix(r)?;
        let nv = r.u64()?;
        let nt = r.u64()?;
        if let Some(volume) = mo.map(|m| &m.volumes[k]) {
            let same = volume.volume_type().value() == ty
                && (0..4).all(|row| {
                    (0..4).all(|col| {
                        volume.get_matrix().m.at(row, col).to_bits() == m.m.at(row, col).to_bits()
                    })
                })
                && volume.mesh().its.vertices.len() as u64 == nv
                && volume.mesh().its.indices.len() as u64 == nt;
            if !same {
                return Err(ReplayError::new(format!(
                    "FOBJ's model volume {k} is not the model's"
                )));
            }
        }
    }
    let regions = if r.boolean()? {
        let trafo_bboxes = matrix(r)?;
        let m = r.count(8)?;
        let mut all_regions = Vec::with_capacity(m);
        for _ in 0..m {
            all_regions.push((r.i32()?, r.i32()?));
        }
        let ids: Vec<ObjectID> =
            mo.map_or_else(Vec::new, |m| m.volumes.iter().map(|v| v.id()).collect());
        let model_ranges = mo.map_or(&[][..], |m| &m.layer_config_ranges[..]);
        let layer_ranges = read_layer_ranges(r, &ids, &all_regions, model_ranges)?;
        Some(FobjRegions {
            trafo_bboxes,
            all_regions,
            layer_ranges,
        })
    } else {
        None
    };
    Ok((object, regions))
}

/// A front-config dump read into a print over `model` (front-objects' model).
///
/// # Errors
///
/// A dump that is not front-config's, a value the data model cannot hold, or one that disagrees
/// with the model.
#[allow(clippy::too_many_lines)]
pub fn read_front_config(bytes: &[u8], model: Model) -> Result<FrontConfig> {
    let (head, run, body) = open(bytes)?;
    let mut confs: Vec<(usize, Conf)> = Vec::new();
    let mut fprt = None;
    let mut fobjs = Vec::new();
    for (i, rec) in body.iter().enumerate() {
        match &rec.kind {
            b"CONF" if fprt.is_none() && fobjs.is_empty() => {
                confs.push((i, read_record(i, rec, read_conf)?));
            }
            b"FPRT" if fprt.is_none() && fobjs.is_empty() => fprt = Some((i, *rec)),
            b"FOBJ" if fprt.is_some() => fobjs.push((i, *rec)),
            _ => {
                return Err(ReplayError::new(format!(
                    "record {} ({}) out of front-config's order",
                    i + 2,
                    rec.kind_name()
                )));
            }
        }
    }
    let Some((fprt_index, fprt)) = fprt else {
        return Err(ReplayError::new("front-config without FPRT"));
    };
    let mut confs = confs.into_iter();
    let mut conf = |scope: &str, object: i64, part: i64| -> Result<Conf> {
        let (i, c) = confs
            .next()
            .ok_or_else(|| ReplayError::new(format!("no CONF {scope} ({object}, {part})")))?;
        if c.scope != scope || c.object != object || c.part != part {
            return Err(scope_error(i, &c, scope));
        }
        Ok(c)
    };
    let job = dynamic_config(&conf("job", -1, -1)?).map_err(|e| e.context("CONF job"))?;
    let mut print = Print::new();
    print.base.base.full_print_config =
        dynamic_config(&conf("print-full", -1, -1)?).map_err(|e| e.context("CONF print-full"))?;
    print.config = static_config(&conf("print", -1, -1)?).map_err(|e| e.context("CONF print"))?;
    print.base.base.model = model;

    // The objects (FOBJ) with their configs (CONF object and region, which come first).
    let mut object_confs = Vec::with_capacity(fobjs.len());
    let mut objects = Vec::with_capacity(fobjs.len());
    let mut fobj_regions = Vec::with_capacity(fobjs.len());
    for (k, (i, rec)) in fobjs.iter().enumerate() {
        let (object, regions) = read_record(*i, rec, |r| read_fobj(r, print.model(), k))?;
        objects.push(object);
        fobj_regions.push(regions);
    }
    for (k, regions) in fobj_regions.iter().enumerate() {
        let object_conf: orca_config::PrintObjectConfig =
            static_config(&conf("object", k as i64, -1)?).map_err(|e| e.context("CONF object"))?;
        let mut region_confs = Vec::new();
        if let Some(regions) = regions {
            for r in 0..regions.all_regions.len() {
                let c: orca_config::PrintRegionConfig =
                    static_config(&conf("region", k as i64, r as i64)?)
                        .map_err(|e| e.context("CONF region"))?;
                region_confs.push(c);
            }
        }
        object_confs.push((object_conf, region_confs));
    }
    print.default_object_config = static_config(&conf("default-object", -1, -1)?)
        .map_err(|e| e.context("CONF default-object"))?;
    print.default_region_config = static_config(&conf("default-region", -1, -1)?)
        .map_err(|e| e.context("CONF default-region"))?;
    if let Some((i, c)) = confs.next() {
        return Err(ReplayError::new(format!(
            "record {}: CONF {} left over",
            i + 2,
            c.scope
        )));
    }

    // The shared regions, one per model object.
    let mut shared: BTreeMap<usize, Arc<PrintObjectRegions>> = BTreeMap::new();
    for ((object, regions), (object_conf, region_confs)) in
        objects.iter_mut().zip(fobj_regions).zip(object_confs)
    {
        object.config = object_conf;
        let Some(regions) = regions else {
            continue;
        };
        let model_object = object.base.base.model_object;
        if let Some(arc) = shared.get(&model_object) {
            object.shared_regions = Some(Arc::clone(arc));
            continue;
        }
        let mut all_regions = Vec::with_capacity(regions.all_regions.len());
        for (&(print_region_id, poid), config) in regions.all_regions.iter().zip(region_confs) {
            let hash = StaticPrintConfig::hash(&config);
            let region = PrintRegion::new(config, hash, poid);
            region
                .print_region_id
                .store(print_region_id, Ordering::Relaxed);
            all_regions.push(region);
        }
        let arc = Arc::new(PrintObjectRegions {
            all_regions,
            layer_ranges: regions.layer_ranges,
            trafo_bboxes: regions.trafo_bboxes,
            cached_volume_ids: Vec::new(),
        });
        object.shared_regions = Some(Arc::clone(&arc));
        shared.insert(model_object, arc);
    }
    print.objects = objects;

    // FPRT.
    let answers =
        read_record(fprt_index, &fprt, |r| {
            print.base.base.plate_index = r.i32()?;
            print.base.base.no_check = r.boolean()?;
            print.is_bbl_printer = r.boolean()?;
            print.origin = Vec3d::new(r.f64()?, r.f64()?, r.f64()?);
            let n = r.count(4)?;
            let mut maps = Vec::with_capacity(n);
            for _ in 0..n {
                maps.push(r.i32()?);
            }
            if maps != print.get_filament_maps() {
                return Err(ReplayError::new(
                    "FPRT's filament maps are not the print config's filament_map",
                ));
            }
            let n = r.count(4)?;
            for _ in 0..n {
                let id = r.i32()?;
                let found = print.objects.iter().find_map(|o| {
                    let regions = o.shared_regions.as_ref()?;
                    let idx = regions
                        .all_regions
                        .iter()
                        .position(|r| r.print_region_id() == id)?;
                    Some(PrintRegionPtr {
                        regions: Arc::clone(regions),
                        idx,
                    })
                });
                print.print_regions.push(found.ok_or_else(|| {
                    ReplayError::new(format!("no object has the print region {id}"))
                })?);
            }
            let answers = PrintAnswers {
                has_wipe_tower: r.boolean()?,
                has_skirt: r.boolean()?,
                has_infinite_skirt: r.boolean()?,
            };
            let _has_brim = r.boolean()?;
            Ok(answers)
        })?;
    Ok(FrontConfig {
        head,
        run,
        job,
        print,
        answers,
    })
}

/// The front-config dump written again from what was read, under its recorded HEAD and RUN.
///
/// # Errors
///
/// An option that does not serialize.
pub fn write_front_config(front: &FrontConfig) -> Result<Vec<u8>> {
    let h = &front.head;
    let mut dump = Dump::with_run(&h.source, &h.name, h.object, h.occurrence, &front.run);
    put_front_config_dump_records(&mut dump, &front.print, &front.job, &front.answers)
        .map_err(|e| ReplayError::new(e.0))?;
    Ok(dump.finish().1)
}

// ---- Object dumps ----

/// An object dump read: its HEAD and RUN, and the index of the print's object it was read into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectDump {
    /// HEAD.
    pub head: Head,
    /// RUN.
    pub run: RunState,
    /// The object's index in the print.
    pub index: usize,
}

fn read_layer(r: &mut Reader, position: usize, region_ids: &[i32]) -> Result<Layer> {
    if r.u64()? != position as u64 {
        return Err(ReplayError::new(format!("LAYR out of order at {position}")));
    }
    let id = r.u64()? as usize;
    let slice_z = r.f64()?;
    let print_z = r.f64()?;
    let height = r.f64()?;
    let mut layer = Layer::new(id, height, print_z, slice_z);
    layer.slicing_errors = r.boolean()?;
    layer.lslices = expolygons(r)?;
    layer.lslices_extrudable = expolygons(r)?;
    layer.loverhangs = expolygons(r)?;
    layer.sharp_tails = expolygons(r)?;
    layer.cantilevers = expolygons(r)?;
    let n = r.count(4)?;
    for _ in 0..n {
        layer.sharp_tails_height.push(r.f32()?);
    }
    let n = r.count(4)?;
    for _ in 0..n {
        let id = r.i32()?;
        let region = region_ids
            .iter()
            .position(|&r| r == id)
            .ok_or_else(|| ReplayError::new(format!("a layer region of print region {id}")))?;
        let mut lr = LayerRegion::new(region);
        lr.slices.surfaces = surfaces(r)?;
        lr.raw_slices = expolygons(r)?;
        lr.fill_surfaces.surfaces = surfaces(r)?;
        lr.fill_expolygons = expolygons(r)?;
        lr.fill_no_overlap_expolygons = expolygons(r)?;
        lr.unsupported_bridge_edges = polylines(r)?;
        lr.thin_fills = collection_entity(r)?;
        lr.perimeters = collection_entity(r)?;
        lr.fills = collection_entity(r)?;
        layer.regions.push(lr);
    }
    Ok(layer)
}

fn read_support_layer(r: &mut Reader, position: usize) -> Result<SupportLayer> {
    if r.u64()? != position as u64 {
        return Err(ReplayError::new(format!("SUPL out of order at {position}")));
    }
    let id = r.u64()? as usize;
    let slice_z = r.f64()?;
    let print_z = r.f64()?;
    let height = r.f64()?;
    let support_type = match r.u32()? {
        0 => SupportInnerType::Normal,
        1 => SupportInnerType::Tree,
        t => return Err(ReplayError::new(format!("a support type of {t}"))),
    };
    let interface_id = r.u64()? as usize;
    let mut layer = SupportLayer::new(id, interface_id, height, print_z, slice_z);
    layer.support_type = support_type;
    layer.layer.lslices = expolygons(r)?;
    layer.support_islands = expolygons(r)?;
    layer.base_areas = expolygons(r)?;
    layer.support_fills = collection_entity(r)?;
    Ok(layer)
}

/// A step warning as recorded: its level, whether it is current, its message and its id.
pub type RecordedWarning = (u8, bool, String, i32);

/// A step as recorded: its state and its warnings.
pub type RecordedStep = (u8, Vec<RecordedWarning>);

/// One step's state and warnings as recorded.
struct RecordedState {
    state: u8,
    warnings: Vec<RecordedWarning>,
}

fn read_step_states(r: &mut Reader) -> Result<Vec<RecordedState>> {
    let n = r.count(9)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let state = r.u8()?;
        let m = r.count(10)?;
        let mut warnings = Vec::with_capacity(m);
        for _ in 0..m {
            warnings.push((r.u8()?, r.boolean()?, r.str()?, r.i32()?));
        }
        out.push(RecordedState { state, warnings });
    }
    Ok(out)
}

fn warning_level(v: u8) -> Result<WarningLevel> {
    match v {
        0 => Ok(WarningLevel::NonCritical),
        1 => Ok(WarningLevel::Critical),
        _ => Err(ReplayError::new(format!("a warning level of {v}"))),
    }
}

/// The step states rebuilt by the state changes that leave them as recorded: a done step is
/// started, given its warnings and set done; a started step is started, given its warnings that
/// are no longer current, started again and given its current ones; an invalid step with
/// warnings is started, given them and invalidated.
///
/// # Errors
///
/// A count that is not the steps', an unknown state or level, or warnings that no sequence of
/// state changes leaves as recorded.
pub fn rebuild_step_states<S: StepType, const COUNT: usize>(
    state: &mut PrintState<S, COUNT>,
    recorded: &[RecordedStep],
) -> Result<()> {
    if recorded.len() != COUNT {
        return Err(ReplayError::new(format!(
            "{} step states for {COUNT} steps",
            recorded.len()
        )));
    }
    *state = PrintState::default();
    for (s, (st, warnings)) in recorded.iter().enumerate() {
        let step = S::from_index(s);
        let add = |state: &mut PrintState<S, COUNT>, current: bool| -> Result<()> {
            for (level, c, message, id) in warnings {
                if *c == current {
                    state.active_step_add_warning(warning_level(*level)?, message, *id);
                }
            }
            Ok(())
        };
        match st {
            0 => {
                if !warnings.is_empty() {
                    state.set_started(step, || {});
                    add(state, false)?;
                    state.invalidate(step, || {});
                }
            }
            1 => {
                state.set_started(step, || {});
                add(state, false)?;
                state.set_started(step, || {});
                add(state, true)?;
            }
            2 => {
                state.set_started(step, || {});
                add(state, true)?;
                state.set_done(step, || {});
            }
            _ => return Err(ReplayError::new(format!("a step state of {st}"))),
        }
        let got = state.state_with_warnings(step);
        let same = crate::dump::state_value(got.state) == *st
            && got.warnings.len() == warnings.len()
            && got
                .warnings
                .iter()
                .zip(warnings)
                .all(|(w, (level, c, message, id))| {
                    crate::dump::warning_level_value(w.level) == *level
                        && w.current == *c
                        && &w.message == message
                        && w.message_id == *id
                });
        if !same {
            return Err(ReplayError::new(format!(
                "step {s}'s warnings are in an order no state changes leave"
            )));
        }
    }
    Ok(())
}

/// An object dump read into the print's object it is of: its OBJ values, layers and support
/// layers, OSTA's state, its step states and LEXT's values.
///
/// # Errors
///
/// A dump that is not an object dump, an object the print does not have, or a value the data
/// model cannot hold.
#[allow(clippy::too_many_lines)]
pub fn read_object_dump(bytes: &[u8], print: &mut Print) -> Result<ObjectDump> {
    let (head, run, body) = open(bytes)?;
    let mut records = body.iter().enumerate().peekable();
    let (i, rec) = records
        .next()
        .ok_or_else(|| ReplayError::new("an object dump without OBJ"))?;
    expect_kind(i, rec, b"OBJ ")?;
    let mut r = Reader::new(rec.payload);
    let index = usize::try_from(r.i64()?)
        .ok()
        .filter(|&k| k < print.objects.len())
        .ok_or_else(|| ReplayError::new("OBJ of an object the print does not have"))?;
    let model_object = print.objects[index].base.base.model_object;
    let volume_ids: Vec<ObjectID> = print
        .model()
        .objects
        .get(model_object)
        .map_or_else(Vec::new, |m| m.volumes.iter().map(|v| v.id()).collect());
    // The layers' regions are those of the layers' object (dump::layer_object): the object whose
    // layers this one shares when OSTA names one (OSTA comes after the layers).
    let layers_of = body
        .iter()
        .find(|r| &r.kind == b"OSTA")
        .and_then(|r| {
            let mut r = Reader::new(r.payload);
            r.i64().ok()?;
            usize::try_from(r.i64().ok()?).ok()
        })
        .filter(|&s| s < print.objects.len())
        .unwrap_or(index);
    let region_ids: Vec<i32> = print.objects[layers_of]
        .shared_regions
        .as_ref()
        .map_or_else(Vec::new, |r| {
            r.all_regions
                .iter()
                .map(PrintRegion::print_region_id)
                .collect()
        });
    let object = &mut print.objects[index];
    let (layer_count, support_count) = read_record(i, rec, |r| {
        r.i64()?;
        let _name = r.str()?;
        object.size = Vec3crd::new(r.i64()?, r.i64()?, r.i64()?);
        object.center_offset = point(r)?;
        object.trafo = matrix(r)?;
        object.max_z = r.f64()?;
        let n = r.count(24)?;
        object.instances.resize_with(n, PrintInstance::default);
        for instance in &mut object.instances {
            instance.shift = point(r)?;
            instance.id = r.u64()? as usize;
        }
        Ok((r.u64()?, r.u64()?))
    })?;
    object.layers.clear();
    object.support_layers.clear();
    while let Some((i, rec)) = records.next_if(|(_, r)| &r.kind == b"LAYR") {
        let position = object.layers.len();
        let layer = read_record(i, rec, |r| read_layer(r, position, &region_ids))?;
        object.layers.push(layer);
    }
    while let Some((i, rec)) = records.next_if(|(_, r)| &r.kind == b"SUPL") {
        let position = object.support_layers.len();
        let layer = read_record(i, rec, |r| read_support_layer(r, position))?;
        object.support_layers.push(layer);
    }
    if object.layers.len() as u64 != layer_count
        || object.support_layers.len() as u64 != support_count
    {
        return Err(ReplayError::new(
            "OBJ's layer counts are not the LAYR and SUPL records'",
        ));
    }
    let (i, rec) = records
        .next()
        .ok_or_else(|| ReplayError::new("an object dump without OSTA"))?;
    expect_kind(i, rec, b"OSTA")?;
    let object_count = print.objects.len();
    let object = &mut print.objects[index];
    read_record(i, rec, |r| {
        if r.i64()? != index as i64 {
            return Err(ReplayError::new("OSTA of another object"));
        }
        let shared = r.i64()?;
        object.shared_object = if shared == -1 {
            None
        } else {
            Some(
                usize::try_from(shared)
                    .ok()
                    .filter(|&s| s < object_count)
                    .ok_or_else(|| ReplayError::new(format!("a shared object of {shared}")))?,
            )
        };
        object.slicing_params = slicing_parameters(r)?;
        let n = r.count(32)?;
        if n != object.instances.len() {
            return Err(ReplayError::new("OSTA's instances are not OBJ's"));
        }
        for instance in &mut object.instances {
            instance.unique_id = r.u64()? as usize;
            instance.model_instance = index_or_max(r.i64()?)?;
            // The shift without the plate offset follows from the shift and the plate's origin.
            point(r)?;
        }
        let n = r.count(4)?;
        object.object_first_layer_wall_extruders.clear();
        for _ in 0..n {
            object.object_first_layer_wall_extruders.push(r.i32()?);
        }
        object.first_layer_object_brim_bounding_box = bbox(r)?;
        object.skirt = collection_entity(r)?;
        let n = r.count(16)?;
        object.first_layer_obj_slice_by_volume.clear();
        for _ in 0..n {
            let id = volume_id(&volume_ids, r.i64()?)?;
            let m = r.count(8)?;
            let mut slices = Vec::with_capacity(m);
            for _ in 0..m {
                slices.push(expolygons(r)?);
            }
            object.first_layer_obj_slice_by_volume.push(VolumeSlices {
                volume_id: id,
                slices,
            });
        }
        let n = r.count(20)?;
        object.first_layer_obj_slice_by_groups.clear();
        for _ in 0..n {
            let group_id = r.i32()?;
            let m = r.count(8)?;
            let mut ids = Vec::with_capacity(m);
            for _ in 0..m {
                ids.push(volume_id(&volume_ids, r.i64()?)?);
            }
            let slices = expolygons(r)?;
            object
                .first_layer_obj_slice_by_groups
                .push(GroupedVolumeSlices {
                    group_id,
                    volume_ids: ids,
                    slices,
                });
        }
        Ok(())
    })?;
    let (i, rec) = records
        .next()
        .ok_or_else(|| ReplayError::new("an object dump without OWRN"))?;
    expect_kind(i, rec, b"OWRN")?;
    read_record(i, rec, |r| {
        if r.i64()? != index as i64 {
            return Err(ReplayError::new("OWRN of another object"));
        }
        let recorded: Vec<RecordedStep> = read_step_states(r)?
            .into_iter()
            .map(|s| (s.state, s.warnings))
            .collect();
        rebuild_step_states(&mut object.base.state, &recorded)
    })?;
    let layers = object.layers.len();
    for position in 0..layers {
        let (i, rec) = records
            .next()
            .ok_or_else(|| ReplayError::new("an object dump with too few LEXT records"))?;
        expect_kind(i, rec, b"LEXT")?;
        let layer = &mut object.layers[position];
        read_record(i, rec, |r| {
            if r.u64()? != position as u64 {
                return Err(ReplayError::new(format!("LEXT out of order at {position}")));
            }
            let neighbour = |v: i64| -> Result<Option<usize>> {
                Ok(match v {
                    -1 => None,
                    v => Some(
                        usize::try_from(v)
                            .ok()
                            .filter(|&v| v < layers)
                            .ok_or_else(|| ReplayError::new(format!("a layer index of {v}")))?,
                    ),
                })
            };
            layer.lower_layer = neighbour(r.i64()?)?;
            layer.upper_layer = neighbour(r.i64()?)?;
            let n = r.count(33)?;
            layer.lslices_bboxes.clear();
            for _ in 0..n {
                layer.lslices_bboxes.push(bbox(r)?);
            }
            layer.loverhangs_bbox = bbox(r)?;
            let n = r.count(36)?;
            layer.curled_lines.clear();
            for _ in 0..n {
                let a = point(r)?;
                let b = point(r)?;
                layer
                    .curled_lines
                    .push(CurledLine::with_height(a, b, r.f32()?));
            }
            Ok(())
        })?;
    }
    if let Some((i, rec)) = records.next() {
        return Err(ReplayError::new(format!(
            "record {} ({}) after the last LEXT",
            i + 2,
            rec.kind_name()
        )));
    }
    Ok(ObjectDump { head, run, index })
}

/// The object dump written again from the print's object, under its recorded HEAD and RUN.
#[must_use]
pub fn write_object_dump(print: &Print, dump: &ObjectDump) -> Vec<u8> {
    let h = &dump.head;
    let mut out = Dump::with_run(&h.source, &h.name, h.object, h.occurrence, &dump.run);
    put_object_dump_records(
        &mut out,
        print,
        &print.objects[dump.index],
        dump.index as i64,
    );
    out.finish().1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dump::{Payload, put_entity, put_step_states};
    use crate::print_object::{POS_COUNT, PrintObjectStep};

    #[test]
    fn entities_read_back_into_their_classes() {
        let mut path = ExtrusionPath::new(ExtrusionRole::ExternalPerimeter, 0.1, 0.45, 0.2);
        path.polyline.points = vec![Point3::new(1, 2, 3), Point3::new(-4, 5, 6)];
        path.inset_idx = 2;
        path.set_reverse();
        let mut c = ExtrusionEntityCollection::default();
        c.entities.push(ExtrusionEntity::Path(path.clone()));
        let mut l = ExtrusionLoop::from_path(&path, ExtrusionLoopRole::DEFAULT);
        l.inset_idx = 1;
        c.entities.push(ExtrusionEntity::Loop(l));
        c.entities
            .push(ExtrusionEntity::PathOriented(ExtrusionPathOriented {
                path: rebuild_reversible(path),
            }));
        c.set_reverse();
        let e = ExtrusionEntity::Collection(c);
        let mut p = Payload::new();
        put_entity(&mut p, &e);
        let mut r = Reader::new(p.bytes());
        let back = entity(&mut r).unwrap();
        r.finish().unwrap();
        let mut q = Payload::new();
        put_entity(&mut q, &back);
        assert_eq!(p, q);
    }

    #[test]
    fn step_states_rebuild_as_recorded() {
        let recorded: Vec<RecordedStep> = (0..POS_COUNT)
            .map(|s| match s {
                7 => (2, vec![(0, true, "support".into(), 2)]),
                3 => (
                    1,
                    vec![(1, false, "old".into(), 0), (0, true, "new".into(), 0)],
                ),
                5 => (0, vec![(0, false, "gone".into(), 4)]),
                _ => (2, vec![]),
            })
            .collect();
        let mut state: PrintState<PrintObjectStep, POS_COUNT> = PrintState::default();
        rebuild_step_states(&mut state, &recorded).unwrap();
        let mut p = Payload::new();
        put_step_states(&mut p, &state);
        let mut r = Reader::new(p.bytes());
        let back: Vec<RecordedStep> = read_step_states(&mut r)
            .unwrap()
            .into_iter()
            .map(|s| (s.state, s.warnings))
            .collect();
        assert_eq!(back, recorded);
        let bad = vec![(
            1u8,
            vec![(0u8, true, "a".to_string(), 0), (0, false, "b".into(), 0)],
        )];
        let mut one: PrintState<PrintObjectStep, 1> = PrintState::default();
        assert!(rebuild_step_states(&mut one, &bad).is_err());
    }

    #[test]
    fn a_conf_option_takes_its_numbers_from_the_bits() {
        let def = orca_config::get("layer_height").unwrap();
        let exact = 0.1f64 + 0.2f64;
        let o = ConfOption {
            key: "layer_height".into(),
            ty: create_default(def).ty().code(),
            text: "0.3".into(),
            floats: vec![exact],
        };
        let v = option_value(create_default(def), &o).unwrap();
        assert_eq!(v, OptionValue::Float(exact));
        let wrong = ConfOption { ty: 0, ..o.clone() };
        assert!(option_value(create_default(def), &wrong).is_err());
    }
}
