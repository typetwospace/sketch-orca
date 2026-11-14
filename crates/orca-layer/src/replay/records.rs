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
// Carries no OrcaSlicer code: it reads every record kind the parity oracle's OracleDump.cpp
// writes (lines 164-361, 415-500, 543-770, 816-903 and 1047-1487, sketch-orca oracle patches 0005,
// 0006 and 0007), field by field in the order parity/SOP1.md lists them. Written on 2026-11-14
// (orca-o3-dump-1).

//! Any dump's records by kind: [`read_records`] reads a dump into [`Record`]s, each its kind and
//! its fields as written ([`Value`]), so that a plate's port can compare what it makes with the
//! oracle's field by field; [`Record::payload`] writes the fields back, and gives the recorded
//! payload's bytes when the reading took every field.
//!
//! The kinds: HEAD, RUN, END and CONF of every dump; front-objects' MODL, PCGC, HRNG, BRIM and
//! PNTV; the object dumps' OBJ, LAYR, SUPL, OSTA, OWRN and LEXT; front-config's FPRT and FOBJ;
//! the print dumps' PRNT, TOOL, WIPE, STAT, CNFL, PWRN, PXTR and TLXT. A field holds a value
//! ([`Value::U8`] to [`Value::Str`]), a list (a u64 count, then the elements) or a group of named
//! fields. A field the writer writes only behind a bool (WIPE's priming and final purge, CNFL's
//! conflict) is in the group only when the bool is true. Floats compare by their bits.
//!
//! A field is named after the C++ member it is written from. An entity is a group whose first
//! field, `kind`, is the entity's kind (`dump::EK_*`).

use super::sop1::{Reader, ReplayError, Result, kind_name, read_container};

/// One field's value as written.
#[derive(Clone, Debug)]
pub enum Value {
    /// A u8.
    U8(u8),
    /// A u16.
    U16(u16),
    /// A u32.
    U32(u32),
    /// A u64.
    U64(u64),
    /// An i32.
    I32(i32),
    /// An i64.
    I64(i64),
    /// A bool.
    Bool(bool),
    /// An f32.
    F32(f32),
    /// An f64.
    F64(f64),
    /// A text's bytes.
    Str(Vec<u8>),
    /// A list: its count, then its elements.
    List(Vec<Value>),
    /// A group of named fields, in their order.
    Group(Vec<(&'static str, Value)>),
}

impl PartialEq for Value {
    /// The same value; floats by their bits.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::U8(a), Self::U8(b)) => a == b,
            (Self::U16(a), Self::U16(b)) => a == b,
            (Self::U32(a), Self::U32(b)) => a == b,
            (Self::U64(a), Self::U64(b)) => a == b,
            (Self::I32(a), Self::I32(b)) => a == b,
            (Self::I64(a), Self::I64(b)) => a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::F32(a), Self::F32(b)) => a.to_bits() == b.to_bits(),
            (Self::F64(a), Self::F64(b)) => a.to_bits() == b.to_bits(),
            (Self::Str(a), Self::Str(b)) => a == b,
            (Self::List(a), Self::List(b)) => a == b,
            (Self::Group(a), Self::Group(b)) => a == b,
            _ => false,
        }
    }
}

impl Value {
    /// The value written as the dump writes it.
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Self::U8(v) => out.push(*v),
            Self::U16(v) => out.extend_from_slice(&v.to_le_bytes()),
            Self::U32(v) => out.extend_from_slice(&v.to_le_bytes()),
            Self::U64(v) => out.extend_from_slice(&v.to_le_bytes()),
            Self::I32(v) => out.extend_from_slice(&v.to_le_bytes()),
            Self::I64(v) => out.extend_from_slice(&v.to_le_bytes()),
            Self::Bool(v) => out.push(u8::from(*v)),
            Self::F32(v) => out.extend_from_slice(&v.to_bits().to_le_bytes()),
            Self::F64(v) => out.extend_from_slice(&v.to_bits().to_le_bytes()),
            Self::Str(v) => {
                out.extend_from_slice(&(v.len() as u32).to_le_bytes());
                out.extend_from_slice(v);
            }
            Self::List(v) => {
                out.extend_from_slice(&(v.len() as u64).to_le_bytes());
                for e in v {
                    e.encode(out);
                }
            }
            Self::Group(v) => {
                for (_, e) in v {
                    e.encode(out);
                }
            }
        }
    }

    /// A group's field of that name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Value> {
        match self {
            Self::Group(fields) => fields.iter().find(|(n, _)| *n == name).map(|(_, v)| v),
            _ => None,
        }
    }

    /// A list's elements.
    #[must_use]
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Self::List(v) => Some(v),
            _ => None,
        }
    }

    /// An integer field as an i128 (any of the integer kinds and a bool).
    #[must_use]
    pub fn as_int(&self) -> Option<i128> {
        Some(match self {
            Self::U8(v) => i128::from(*v),
            Self::U16(v) => i128::from(*v),
            Self::U32(v) => i128::from(*v),
            Self::U64(v) => i128::from(*v),
            Self::I32(v) => i128::from(*v),
            Self::I64(v) => i128::from(*v),
            Self::Bool(v) => i128::from(*v),
            _ => return None,
        })
    }

    /// A text field's bytes.
    #[must_use]
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Str(v) => Some(v),
            _ => None,
        }
    }
}

/// One record: its kind and its fields as written.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    /// The kind.
    pub kind: [u8; 4],
    /// The fields, in their order.
    pub fields: Vec<(&'static str, Value)>,
}

impl Record {
    /// The kind as text.
    #[must_use]
    pub fn kind_name(&self) -> String {
        kind_name(&self.kind)
    }

    /// The field of that name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.fields.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
    }

    /// The fields written again as the payload.
    #[must_use]
    pub fn payload(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (_, v) in &self.fields {
            v.encode(&mut out);
        }
        out
    }
}

/// Every record of a dump, END included, read by its kind.
///
/// # Errors
///
/// A broken container, a kind this reader does not know, or a payload that does not read as its
/// kind's fields to its last byte.
pub fn read_records(bytes: &[u8]) -> Result<Vec<Record>> {
    let raw = read_container(bytes)?;
    raw.iter()
        .enumerate()
        .map(|(i, r)| {
            decode_record(&r.kind, r.payload)
                .map_err(|e| e.context(&format!("record {i} ({})", r.kind_name())))
        })
        .collect()
}

/// One record's fields, read by its kind.
///
/// # Errors
///
/// A kind this reader does not know, or a payload that does not read as its fields to its end.
pub fn decode_record(kind: &[u8; 4], payload: &[u8]) -> Result<Record> {
    let mut r = Reader::new(payload);
    let fields = match kind {
        b"HEAD" => head(&mut r)?,
        b"RUN " => run(&mut r)?,
        b"END " => vec![("records", u64v(&mut r)?)],
        b"CONF" => conf(&mut r)?,
        b"MODL" => modl(&mut r)?,
        b"PCGC" => pcgc(&mut r)?,
        b"HRNG" => hrng(&mut r)?,
        b"BRIM" => brim(&mut r)?,
        b"PNTV" => pntv(&mut r)?,
        b"OBJ " => obj(&mut r)?,
        b"LAYR" => layr(&mut r)?,
        b"SUPL" => supl(&mut r)?,
        b"OSTA" => osta(&mut r)?,
        b"OWRN" => vec![("index", i64v(&mut r)?), ("states", step_states(&mut r)?)],
        b"LEXT" => lext(&mut r)?,
        b"FPRT" => fprt(&mut r)?,
        b"FOBJ" => fobj(&mut r)?,
        b"PRNT" => prnt(&mut r)?,
        b"TOOL" => tool(&mut r)?,
        b"WIPE" => wipe(&mut r)?,
        b"STAT" => stat(&mut r)?,
        b"CNFL" => cnfl(&mut r)?,
        b"PWRN" => vec![("states", step_states(&mut r)?)],
        b"PXTR" => pxtr(&mut r)?,
        b"TLXT" => vec![("layers", list(&mut r, |r| list(r, custom_gcode))?)],
        _ => {
            return Err(ReplayError::new(format!(
                "a record of the unknown kind {:?}",
                kind_name(kind)
            )));
        }
    };
    r.finish()?;
    Ok(Record {
        kind: *kind,
        fields,
    })
}

// ---- Values ----

type Fields = Vec<(&'static str, Value)>;

fn u8v(r: &mut Reader) -> Result<Value> {
    Ok(Value::U8(r.u8()?))
}
fn u16v(r: &mut Reader) -> Result<Value> {
    Ok(Value::U16(r.u16()?))
}
fn u32v(r: &mut Reader) -> Result<Value> {
    Ok(Value::U32(r.u32()?))
}
fn u64v(r: &mut Reader) -> Result<Value> {
    Ok(Value::U64(r.u64()?))
}
fn i32v(r: &mut Reader) -> Result<Value> {
    Ok(Value::I32(r.i32()?))
}
fn i64v(r: &mut Reader) -> Result<Value> {
    Ok(Value::I64(r.i64()?))
}
fn boolv(r: &mut Reader) -> Result<Value> {
    Ok(Value::Bool(r.boolean()?))
}
fn f32v(r: &mut Reader) -> Result<Value> {
    Ok(Value::F32(r.f32()?))
}
fn f64v(r: &mut Reader) -> Result<Value> {
    Ok(Value::F64(r.f64()?))
}
fn strv(r: &mut Reader) -> Result<Value> {
    Ok(Value::Str(r.str_bytes()?.to_vec()))
}

/// A list: its count, then each element.
fn list<'a>(
    r: &mut Reader<'a>,
    mut element: impl FnMut(&mut Reader<'a>) -> Result<Value>,
) -> Result<Value> {
    let n = r.count(1)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(element(r)?);
    }
    Ok(Value::List(out))
}

fn group(fields: Fields) -> Value {
    Value::Group(fields)
}

/// A matrix's elements by row and column, column by column as the dump writes them.
const MATRIX: [&str; 16] = [
    "r0c0", "r1c0", "r2c0", "r3c0", "r0c1", "r1c1", "r2c1", "r3c1", "r0c2", "r1c2", "r2c2", "r3c2",
    "r0c3", "r1c3", "r2c3", "r3c3",
];

/// A 4 x 4 matrix, column by column with no count: a group of its sixteen elements.
fn matrix(r: &mut Reader) -> Result<Value> {
    let mut out = Vec::with_capacity(16);
    for name in MATRIX {
        out.push((name, Value::F64(r.f64()?)));
    }
    Ok(group(out))
}

// ---- Geometry and extrusions ----

fn point(r: &mut Reader) -> Result<Value> {
    Ok(group(vec![("x", i64v(r)?), ("y", i64v(r)?)]))
}

fn points(r: &mut Reader) -> Result<Value> {
    list(r, point)
}

fn expolygon(r: &mut Reader) -> Result<Value> {
    Ok(group(vec![
        ("contour", points(r)?),
        ("holes", list(r, points)?),
    ]))
}

fn expolygons(r: &mut Reader) -> Result<Value> {
    list(r, expolygon)
}

fn bbox(r: &mut Reader) -> Result<Value> {
    Ok(group(vec![
        ("defined", boolv(r)?),
        ("min", point(r)?),
        ("max", point(r)?),
    ]))
}

fn fitting(r: &mut Reader) -> Result<Value> {
    list(r, |r| {
        Ok(group(vec![
            ("start_point_index", u64v(r)?),
            ("end_point_index", u64v(r)?),
            ("path_type", u8v(r)?),
            ("is_arc", boolv(r)?),
            ("center", point(r)?),
            ("radius", f64v(r)?),
            ("length", f64v(r)?),
            ("angle_radians", f64v(r)?),
            ("polar_start_theta", f64v(r)?),
            ("polar_end_theta", f64v(r)?),
            ("start_point", point(r)?),
            ("end_point", point(r)?),
            ("direction", u8v(r)?),
        ]))
    })
}

fn polylines(r: &mut Reader) -> Result<Value> {
    list(r, |r| {
        Ok(group(vec![
            ("points", points(r)?),
            ("fitting_result", fitting(r)?),
        ]))
    })
}

fn surfaces(r: &mut Reader) -> Result<Value> {
    list(r, |r| {
        Ok(group(vec![
            ("surface_type", u32v(r)?),
            ("expolygon", expolygon(r)?),
            ("thickness", f64v(r)?),
            ("thickness_layers", u16v(r)?),
            ("bridge_angle", f64v(r)?),
            ("extra_perimeters", u16v(r)?),
        ]))
    })
}

fn path_fields(r: &mut Reader, f: &mut Fields) -> Result<()> {
    f.push(("role", u8v(r)?));
    f.push(("inset_idx", i32v(r)?));
    f.push((
        "points",
        list(r, |r| {
            Ok(group(vec![
                ("x", i64v(r)?),
                ("y", i64v(r)?),
                ("z", i64v(r)?),
            ]))
        })?,
    ));
    f.push(("fitting_result", fitting(r)?));
    f.push(("overhang_degree", f64v(r)?));
    f.push(("curve_degree", i32v(r)?));
    f.push(("mm3_per_mm", f64v(r)?));
    f.push(("width", f32v(r)?));
    f.push(("height", f32v(r)?));
    f.push(("smooth_speed", f64v(r)?));
    f.push(("z_contoured", boolv(r)?));
    f.push(("is_force_no_extrusion", boolv(r)?));
    f.push(("can_reverse", boolv(r)?));
    Ok(())
}

fn loop_fields(r: &mut Reader, f: &mut Fields) -> Result<()> {
    f.push(("inset_idx", i32v(r)?));
    f.push(("loop_role", u8v(r)?));
    f.push(("paths", list(r, entity)?));
    Ok(())
}

/// An entity: its kind, then its class's fields.
fn entity(r: &mut Reader) -> Result<Value> {
    let at = r.position();
    let kind = r.u8()?;
    let mut f: Fields = vec![("kind", Value::U8(kind))];
    match kind {
        1 | 4 => path_fields(r, &mut f)?,
        2 => {
            path_fields(r, &mut f)?;
            f.push(("z_diffs", list(r, f64v)?));
        }
        3 => {
            path_fields(r, &mut f)?;
            f.push(("slope_begin_z_ratio", f64v(r)?));
            f.push(("slope_begin_e_ratio", f64v(r)?));
            f.push(("slope_end_z_ratio", f64v(r)?));
            f.push(("slope_end_e_ratio", f64v(r)?));
        }
        5 => {
            f.push(("inset_idx", i32v(r)?));
            f.push(("can_reverse", boolv(r)?));
            f.push(("paths", list(r, entity)?));
        }
        6 => loop_fields(r, &mut f)?,
        7 => {
            loop_fields(r, &mut f)?;
            f.push(("starts", list(r, entity)?));
            f.push(("ends", list(r, entity)?));
        }
        8 => {
            f.push(("inset_idx", i32v(r)?));
            f.push(("no_sort", boolv(r)?));
            f.push(("can_reverse", boolv(r)?));
            f.push(("entities", list(r, entity)?));
        }
        0 => f.push(("type_name", strv(r)?)),
        k => {
            return Err(ReplayError::new(format!(
                "an entity of the unknown kind {k} (at byte {at})"
            )));
        }
    }
    Ok(group(f))
}

// ---- Shared records ----

fn head(r: &mut Reader) -> Result<Fields> {
    Ok(vec![
        ("source", strv(r)?),
        ("name", strv(r)?),
        ("object", i64v(r)?),
        ("occurrence", u32v(r)?),
    ])
}

fn run(r: &mut Reader) -> Result<Fields> {
    Ok(vec![
        (
            "knobs",
            list(r, |r| {
                Ok(group(vec![
                    ("name", strv(r)?),
                    ("set", boolv(r)?),
                    ("value", strv(r)?),
                ]))
            })?,
        ),
        ("max_concurrency", i32v(r)?),
        ("max_allowed_parallelism", u64v(r)?),
        ("fma3_enable", i32v(r)?),
    ])
}

fn conf(r: &mut Reader) -> Result<Fields> {
    Ok(vec![
        ("scope", strv(r)?),
        ("object", i64v(r)?),
        ("part", i64v(r)?),
        (
            "options",
            list(r, |r| {
                Ok(group(vec![
                    ("key", strv(r)?),
                    ("type", u32v(r)?),
                    ("serialize", strv(r)?),
                    ("floats", list(r, f64v)?),
                ]))
            })?,
        ),
    ])
}

fn step_states(r: &mut Reader) -> Result<Value> {
    list(r, |r| {
        Ok(group(vec![
            ("state", u8v(r)?),
            (
                "warnings",
                list(r, |r| {
                    Ok(group(vec![
                        ("level", u8v(r)?),
                        ("current", boolv(r)?),
                        ("message", strv(r)?),
                        ("message_id", i32v(r)?),
                    ]))
                })?,
            ),
        ]))
    })
}

fn slicing_parameters(r: &mut Reader) -> Result<Value> {
    let mut f: Fields = vec![
        ("valid", boolv(r)?),
        ("base_raft_layers", u64v(r)?),
        ("interface_raft_layers", u64v(r)?),
    ];
    for name in [
        "base_raft_layer_height",
        "interface_raft_layer_height",
        "contact_raft_layer_height",
        "layer_height",
        "min_layer_height",
        "max_layer_height",
        "max_suport_layer_height",
        "first_print_layer_height",
        "first_object_layer_height",
    ] {
        f.push((name, f64v(r)?));
    }
    for name in [
        "first_object_layer_bridging",
        "zero_gap_interface_top",
        "zero_gap_interface_bottom",
        "zero_gap_interface_raft",
    ] {
        f.push((name, boolv(r)?));
    }
    for name in [
        "gap_raft_object",
        "gap_object_support",
        "gap_support_object",
        "raft_base_top_z",
        "raft_interface_top_z",
        "raft_contact_top_z",
        "object_print_z_min",
        "object_print_z_max",
        "object_print_z_uncompensated_max",
        "object_shrinkage_compensation_z",
    ] {
        f.push((name, f64v(r)?));
    }
    Ok(group(f))
}

fn custom_gcode(r: &mut Reader) -> Result<Value> {
    Ok(group(vec![
        ("print_z", f64v(r)?),
        ("type", i32v(r)?),
        ("extruder", i32v(r)?),
        ("color", strv(r)?),
        ("extra", strv(r)?),
    ]))
}

// ---- front-objects ----

fn modl(r: &mut Reader) -> Result<Fields> {
    Ok(vec![
        ("index", i64v(r)?),
        ("name", strv(r)?),
        ("printable", boolv(r)?),
        ("layer_height_profile", list(r, f64v)?),
        (
            "instances",
            list(r, |r| {
                Ok(group(vec![
                    ("matrix", matrix(r)?),
                    ("printable", boolv(r)?),
                ]))
            })?,
        ),
        (
            "volumes",
            list(r, |r| {
                Ok(group(vec![
                    ("name", strv(r)?),
                    ("type", i32v(r)?),
                    ("matrix", matrix(r)?),
                    (
                        "vertices",
                        list(r, |r| {
                            Ok(group(vec![
                                ("x", f32v(r)?),
                                ("y", f32v(r)?),
                                ("z", f32v(r)?),
                            ]))
                        })?,
                    ),
                    (
                        "indices",
                        list(r, |r| {
                            Ok(group(vec![
                                ("a", i32v(r)?),
                                ("b", i32v(r)?),
                                ("c", i32v(r)?),
                            ]))
                        })?,
                    ),
                ]))
            })?,
        ),
    ])
}

fn pcgc(r: &mut Reader) -> Result<Fields> {
    Ok(vec![
        ("curr_plate_index", i32v(r)?),
        (
            "plates",
            list(r, |r| {
                Ok(group(vec![
                    ("plate", i32v(r)?),
                    ("mode", i32v(r)?),
                    ("gcodes", list(r, custom_gcode)?),
                ]))
            })?,
        ),
    ])
}

fn hrng(r: &mut Reader) -> Result<Fields> {
    Ok(vec![
        ("index", i64v(r)?),
        (
            "ranges",
            list(r, |r| Ok(group(vec![("from", f64v(r)?), ("to", f64v(r)?)])))?,
        ),
    ])
}

fn brim(r: &mut Reader) -> Result<Fields> {
    Ok(vec![
        ("index", i64v(r)?),
        (
            "brim_points",
            list(r, |r| {
                Ok(group(vec![
                    ("x", f32v(r)?),
                    ("y", f32v(r)?),
                    ("z", f32v(r)?),
                    ("head_front_radius", f32v(r)?),
                ]))
            })?,
        ),
    ])
}

fn painting(r: &mut Reader) -> Result<Value> {
    Ok(group(vec![
        (
            "triangles_to_split",
            list(r, |r| {
                Ok(group(vec![
                    ("triangle_idx", i32v(r)?),
                    ("bitstream_start_idx", i32v(r)?),
                ]))
            })?,
        ),
        ("bitstream_size", u64v(r)?),
        ("bitstream_bytes", list(r, u8v)?),
        ("used_states", list(r, boolv)?),
    ]))
}

fn pntv(r: &mut Reader) -> Result<Fields> {
    Ok(vec![
        ("object", i64v(r)?),
        ("volume", i64v(r)?),
        ("supported_facets", painting(r)?),
        ("seam_facets", painting(r)?),
        ("mmu_segmentation_facets", painting(r)?),
        ("fuzzy_skin_facets", painting(r)?),
    ])
}

// ---- Object dumps ----

fn obj(r: &mut Reader) -> Result<Fields> {
    Ok(vec![
        ("index", i64v(r)?),
        ("name", strv(r)?),
        ("size_x", i64v(r)?),
        ("size_y", i64v(r)?),
        ("size_z", i64v(r)?),
        ("center_offset", point(r)?),
        ("trafo", matrix(r)?),
        ("max_z", f64v(r)?),
        (
            "instances",
            list(r, |r| {
                Ok(group(vec![("shift", point(r)?), ("id", u64v(r)?)]))
            })?,
        ),
        ("layer_count", u64v(r)?),
        ("support_layer_count", u64v(r)?),
    ])
}

fn layr(r: &mut Reader) -> Result<Fields> {
    Ok(vec![
        ("position", u64v(r)?),
        ("id", u64v(r)?),
        ("slice_z", f64v(r)?),
        ("print_z", f64v(r)?),
        ("height", f64v(r)?),
        ("slicing_errors", boolv(r)?),
        ("lslices", expolygons(r)?),
        ("lslices_extrudable", expolygons(r)?),
        ("loverhangs", expolygons(r)?),
        ("sharp_tails", expolygons(r)?),
        ("cantilevers", expolygons(r)?),
        ("sharp_tails_height", list(r, f32v)?),
        (
            "regions",
            list(r, |r| {
                Ok(group(vec![
                    ("print_region_id", i32v(r)?),
                    ("slices", surfaces(r)?),
                    ("raw_slices", expolygons(r)?),
                    ("fill_surfaces", surfaces(r)?),
                    ("fill_expolygons", expolygons(r)?),
                    ("fill_no_overlap_expolygons", expolygons(r)?),
                    ("unsupported_bridge_edges", polylines(r)?),
                    ("thin_fills", entity(r)?),
                    ("perimeters", entity(r)?),
                    ("fills", entity(r)?),
                ]))
            })?,
        ),
    ])
}

fn supl(r: &mut Reader) -> Result<Fields> {
    Ok(vec![
        ("position", u64v(r)?),
        ("id", u64v(r)?),
        ("slice_z", f64v(r)?),
        ("print_z", f64v(r)?),
        ("height", f64v(r)?),
        ("support_type", u32v(r)?),
        ("interface_id", u64v(r)?),
        ("lslices", expolygons(r)?),
        ("support_islands", expolygons(r)?),
        ("base_areas", expolygons(r)?),
        ("support_fills", entity(r)?),
    ])
}

fn osta(r: &mut Reader) -> Result<Fields> {
    Ok(vec![
        ("index", i64v(r)?),
        ("shared_object", i64v(r)?),
        ("slicing_parameters", slicing_parameters(r)?),
        (
            "instances",
            list(r, |r| {
                Ok(group(vec![
                    ("unique_id", u64v(r)?),
                    ("model_instance", i64v(r)?),
                    ("shift_without_plate_offset", point(r)?),
                ]))
            })?,
        ),
        ("object_first_layer_wall_extruders", list(r, i32v)?),
        ("first_layer_object_brim_bounding_box", bbox(r)?),
        ("object_skirt", entity(r)?),
        (
            "first_layer_obj_slice",
            list(r, |r| {
                Ok(group(vec![
                    ("volume", i64v(r)?),
                    ("slices", list(r, expolygons)?),
                ]))
            })?,
        ),
        (
            "first_layer_obj_groups",
            list(r, |r| {
                Ok(group(vec![
                    ("group_id", i32v(r)?),
                    ("volumes", list(r, i64v)?),
                    ("slices", expolygons(r)?),
                ]))
            })?,
        ),
    ])
}

fn lext(r: &mut Reader) -> Result<Fields> {
    Ok(vec![
        ("position", u64v(r)?),
        ("lower_layer", i64v(r)?),
        ("upper_layer", i64v(r)?),
        ("lslices_bboxes", list(r, bbox)?),
        ("loverhangs_bbox", bbox(r)?),
        (
            "curled_lines",
            list(r, |r| {
                Ok(group(vec![
                    ("a", point(r)?),
                    ("b", point(r)?),
                    ("curled_height", f32v(r)?),
                ]))
            })?,
        ),
    ])
}

// ---- front-config ----

fn fprt(r: &mut Reader) -> Result<Fields> {
    Ok(vec![
        ("plate_index", i32v(r)?),
        ("no_check", boolv(r)?),
        ("is_bbl_printer", boolv(r)?),
        ("origin_x", f64v(r)?),
        ("origin_y", f64v(r)?),
        ("origin_z", f64v(r)?),
        ("filament_maps", list(r, i32v)?),
        ("print_region_ids", list(r, i32v)?),
        ("has_wipe_tower", boolv(r)?),
        ("has_skirt", boolv(r)?),
        ("has_infinite_skirt", boolv(r)?),
        ("has_brim", boolv(r)?),
    ])
}

fn layer_ranges(r: &mut Reader) -> Result<Value> {
    list(r, |r| {
        Ok(group(vec![
            ("from", f64v(r)?),
            ("to", f64v(r)?),
            ("has_config", boolv(r)?),
            (
                "volumes",
                list(r, |r| {
                    Ok(group(vec![
                        ("volume", i64v(r)?),
                        ("min_x", f32v(r)?),
                        ("min_y", f32v(r)?),
                        ("min_z", f32v(r)?),
                        ("max_x", f32v(r)?),
                        ("max_y", f32v(r)?),
                        ("max_z", f32v(r)?),
                    ]))
                })?,
            ),
            (
                "volume_regions",
                list(r, |r| {
                    Ok(group(vec![
                        ("model_volume", i64v(r)?),
                        ("parent", i32v(r)?),
                        ("region", i32v(r)?),
                    ]))
                })?,
            ),
            (
                "painted_regions",
                list(r, |r| {
                    Ok(group(vec![
                        ("extruder_id", u32v(r)?),
                        ("parent", i32v(r)?),
                        ("region", i32v(r)?),
                    ]))
                })?,
            ),
            (
                "fuzzy_skin_painted_regions",
                list(r, |r| {
                    Ok(group(vec![
                        ("parent_type", u8v(r)?),
                        ("parent", i32v(r)?),
                        ("region", i32v(r)?),
                    ]))
                })?,
            ),
        ]))
    })
}

fn fobj(r: &mut Reader) -> Result<Fields> {
    let mut f: Fields = vec![
        ("index", i64v(r)?),
        ("model_object", i64v(r)?),
        ("size_x", i64v(r)?),
        ("size_y", i64v(r)?),
        ("size_z", i64v(r)?),
        ("center_offset", point(r)?),
        ("trafo", matrix(r)?),
        ("max_z", f64v(r)?),
        (
            "instances",
            list(r, |r| {
                Ok(group(vec![
                    ("shift", point(r)?),
                    ("id", u64v(r)?),
                    ("unique_id", u64v(r)?),
                    ("model_instance", i64v(r)?),
                ]))
            })?,
        ),
        ("slicing_parameters", slicing_parameters(r)?),
        (
            "model_volumes",
            list(r, |r| {
                Ok(group(vec![
                    ("type", i32v(r)?),
                    ("matrix", matrix(r)?),
                    ("vertex_count", u64v(r)?),
                    ("triangle_count", u64v(r)?),
                ]))
            })?,
        ),
    ];
    let has_regions = r.boolean()?;
    f.push(("has_regions", Value::Bool(has_regions)));
    if has_regions {
        f.push(("trafo_bboxes", matrix(r)?));
        f.push((
            "all_regions",
            list(r, |r| {
                Ok(group(vec![
                    ("print_region_id", i32v(r)?),
                    ("print_object_region_id", i32v(r)?),
                ]))
            })?,
        ));
        f.push(("layer_ranges", layer_ranges(r)?));
    }
    Ok(f)
}

// ---- Print dumps ----

fn object_ids(r: &mut Reader) -> Result<Value> {
    list(r, i64v)
}

fn entity_map(r: &mut Reader) -> Result<Value> {
    list(r, |r| {
        Ok(group(vec![("object", i64v(r)?), ("entity", entity(r)?)]))
    })
}

fn prnt(r: &mut Reader) -> Result<Fields> {
    Ok(vec![
        ("skirt", entity(r)?),
        (
            "skirt_brim_groups",
            list(r, |r| {
                Ok(group(vec![
                    ("skirt", entity(r)?),
                    ("object_ids", object_ids(r)?),
                    (
                        "brims",
                        list(r, |r| {
                            Ok(group(vec![
                                ("brim", entity(r)?),
                                ("object_ids", object_ids(r)?),
                            ]))
                        })?,
                    ),
                ]))
            })?,
        ),
        ("has_shared_per_object_skirt", boolv(r)?),
        ("brim_map", entity_map(r)?),
        ("support_brim_map", entity_map(r)?),
        ("first_layer_convex_hull", points(r)?),
        ("skirt_convex_hull", points(r)?),
        ("slice_used_filaments", list(r, u32v)?),
        ("slice_used_filaments_first_layer", list(r, u32v)?),
        ("is_support_used", boolv(r)?),
    ])
}

fn tool(r: &mut Reader) -> Result<Fields> {
    Ok(vec![
        ("first_extruder", u32v(r)?),
        ("last_extruder", u32v(r)?),
        ("all_extruders", list(r, u32v)?),
        (
            "layers",
            list(r, |r| {
                Ok(group(vec![
                    ("print_z", f64v(r)?),
                    ("has_object", boolv(r)?),
                    ("has_support", boolv(r)?),
                    ("extruders", list(r, u32v)?),
                    ("extruder_override", u32v(r)?),
                    ("has_skirt", boolv(r)?),
                    ("has_wipe_tower", boolv(r)?),
                    ("wipe_tower_partitions", u64v(r)?),
                    ("wipe_tower_layer_height", f64v(r)?),
                    ("has_custom_gcode", boolv(r)?),
                ]))
            })?,
        ),
    ])
}

fn xy_f32(r: &mut Reader) -> Result<Value> {
    Ok(group(vec![("x", f32v(r)?), ("y", f32v(r)?)]))
}

fn tool_change(r: &mut Reader) -> Result<Value> {
    Ok(group(vec![
        ("print_z", f32v(r)?),
        ("layer_height", f32v(r)?),
        ("gcode", strv(r)?),
        (
            "extrusions",
            list(r, |r| {
                Ok(group(vec![
                    ("x", f32v(r)?),
                    ("y", f32v(r)?),
                    ("width", f32v(r)?),
                    ("tool", u32v(r)?),
                ]))
            })?,
        ),
        ("start_pos", xy_f32(r)?),
        ("end_pos", xy_f32(r)?),
        ("elapsed_time", f32v(r)?),
        ("priming", boolv(r)?),
        ("is_tool_change", boolv(r)?),
        ("is_contact", boolv(r)?),
        ("tool_change_start_pos", xy_f32(r)?),
        ("wipe_path", list(r, xy_f32)?),
        ("purge_volume", f32v(r)?),
        ("initial_tool", i32v(r)?),
        ("new_tool", i32v(r)?),
        ("is_finish_first", boolv(r)?),
    ]))
}

fn z_and_depth(r: &mut Reader) -> Result<Value> {
    list(r, |r| Ok(group(vec![("z", f32v(r)?), ("depth", f32v(r)?)])))
}

fn wipe(r: &mut Reader) -> Result<Fields> {
    let mut f: Fields = vec![
        ("number_of_toolchanges", i32v(r)?),
        ("depth", f32v(r)?),
        ("brim_width", f32v(r)?),
        ("used_filament", list(r, f32v)?),
        ("z_and_depth_pairs", z_and_depth(r)?),
        ("bbx_defined", boolv(r)?),
        ("bbx_min_x", f64v(r)?),
        ("bbx_min_y", f64v(r)?),
        ("bbx_max_x", f64v(r)?),
        ("bbx_max_y", f64v(r)?),
        ("rib_offset", xy_f32(r)?),
    ];
    let has_priming = r.boolean()?;
    f.push(("has_priming", Value::Bool(has_priming)));
    if has_priming {
        f.push(("priming", list(r, tool_change)?));
    }
    f.push(("tool_changes", list(r, |r| list(r, tool_change))?));
    let has_final_purge = r.boolean()?;
    f.push(("has_final_purge", Value::Bool(has_final_purge)));
    if has_final_purge {
        f.push(("final_purge", tool_change(r)?));
    }
    Ok(f)
}

fn stat(r: &mut Reader) -> Result<Fields> {
    Ok(vec![
        ("estimated_normal_print_time", strv(r)?),
        ("estimated_silent_print_time", strv(r)?),
        ("total_used_filament", f64v(r)?),
        ("total_extruded_volume", f64v(r)?),
        ("total_cost", f64v(r)?),
        ("total_toolchanges", i32v(r)?),
        ("total_weight", f64v(r)?),
        ("total_wipe_tower_cost", f64v(r)?),
        ("total_wipe_tower_filament", f64v(r)?),
        ("initial_tool", u32v(r)?),
        (
            "filament_stats",
            list(r, |r| {
                Ok(group(vec![("extruder", u64v(r)?), ("used", f64v(r)?)]))
            })?,
        ),
    ])
}

fn cnfl(r: &mut Reader) -> Result<Fields> {
    let has = r.boolean()?;
    let mut f: Fields = vec![("has_conflict", Value::Bool(has))];
    if has {
        f.push(("obj_name1", strv(r)?));
        f.push(("obj_name2", strv(r)?));
        f.push(("height", f64v(r)?));
        f.push(("layer", i32v(r)?));
    }
    Ok(f)
}

fn conflict_side(r: &mut Reader) -> Result<Value> {
    Ok(group(vec![("object", i64v(r)?), ("instance", i64v(r)?)]))
}

fn change_stats(r: &mut Reader) -> Result<Value> {
    Ok(group(vec![
        ("filament_flush_weight", i32v(r)?),
        ("filament_change_count", i32v(r)?),
        ("extruder_change_count", i32v(r)?),
    ]))
}

fn pxtr(r: &mut Reader) -> Result<Fields> {
    Ok(vec![
        ("has_conflict", boolv(r)?),
        ("obj1", conflict_side(r)?),
        ("obj2", conflict_side(r)?),
        (
            "objects",
            list(r, |r| {
                Ok(group(vec![
                    ("index", i64v(r)?),
                    ("object_first_layer_wall_extruders", list(r, i32v)?),
                    ("first_layer_object_brim_bounding_box", bbox(r)?),
                    ("object_skirt", entity(r)?),
                ]))
            })?,
        ),
        (
            "geometric_unprintable_filaments",
            list(r, |r| list(r, i32v))?,
        ),
        ("filament_maps", list(r, i32v)?),
        ("stats_by_single_extruder", change_stats(r)?),
        ("stats_by_multi_extruder_best", change_stats(r)?),
        ("stats_by_multi_extruder_curr", change_stats(r)?),
        (
            "fake_wipe_tower",
            group(vec![
                ("pos", xy_f32(r)?),
                ("width", f32v(r)?),
                ("height", f32v(r)?),
                ("layer_height", f32v(r)?),
                ("depth", f32v(r)?),
                ("z_and_depth_pairs", z_and_depth(r)?),
                ("brim_width", f32v(r)?),
                ("rotation_angle", f32v(r)?),
                ("cone_angle", f32v(r)?),
                ("plate_origin_x", f64v(r)?),
                ("plate_origin_y", f64v(r)?),
                ("rib_offset", xy_f32(r)?),
                (
                    "outer_wall",
                    list(r, |r| {
                        Ok(group(vec![("z", f32v(r)?), ("polylines", polylines(r)?)]))
                    })?,
                ),
            ]),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dump::{Dump, Payload, RunState, put_entity};
    use crate::entity::ExtrusionEntity;
    use crate::path::ExtrusionPath;
    use crate::role::ExtrusionRole;

    #[test]
    fn records_read_by_kind_write_their_payloads_back() {
        let mut dump = Dump::with_run("print", "psSkirtBrim", -1, 2, &RunState::current());
        let mut cnfl = Payload::new();
        cnfl.boolean(true);
        cnfl.str("a");
        cnfl.str("b");
        cnfl.f64(1.5);
        cnfl.i32(3);
        dump.record(b"CNFL", &cnfl);
        let (_, bytes) = dump.finish();
        let records = read_records(&bytes).unwrap();
        assert_eq!(records.len(), 4);
        assert_eq!(records[0].get("occurrence"), Some(&Value::U32(2)));
        assert_eq!(records[2].get("layer"), Some(&Value::I32(3)));
        let raw = read_container(&bytes).unwrap();
        for (record, raw) in records.iter().zip(&raw) {
            assert_eq!(record.payload(), raw.payload);
        }
    }

    #[test]
    fn an_entity_reads_as_its_kind_and_fields() {
        let path = ExtrusionPath::new(ExtrusionRole::Perimeter, 0.5, 0.4, 0.2);
        let mut p = Payload::new();
        put_entity(&mut p, &ExtrusionEntity::Path(path));
        let mut r = Reader::new(p.bytes());
        let v = entity(&mut r).unwrap();
        r.finish().unwrap();
        assert_eq!(v.get("kind"), Some(&Value::U8(1)));
        assert_eq!(v.get("width"), Some(&Value::F32(0.4)));
        let mut out = Vec::new();
        v.encode(&mut out);
        assert_eq!(out, p.bytes());
    }

    #[test]
    fn a_matrix_is_its_sixteen_elements_with_no_count() {
        let mut p = Payload::new();
        for k in 0..16u32 {
            p.f64(f64::from(k));
        }
        let mut r = Reader::new(p.bytes());
        let v = matrix(&mut r).unwrap();
        r.finish().unwrap();
        assert_eq!(v.get("r1c0"), Some(&Value::F64(1.0)));
        assert_eq!(v.get("r0c1"), Some(&Value::F64(4.0)));
        let mut out = Vec::new();
        v.encode(&mut out);
        assert_eq!(out, p.bytes());
    }

    #[test]
    fn an_unknown_kind_or_a_short_payload_is_refused() {
        assert!(decode_record(b"ZZZZ", &[]).is_err());
        assert!(decode_record(b"END ", &[1, 2]).is_err());
        assert!(decode_record(b"END ", &[0; 9]).is_err());
    }
}
