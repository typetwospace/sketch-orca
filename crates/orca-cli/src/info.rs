// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! An info job's answer, the result's `info` (`info_json` of `cli/run.cpp`, translated on
//! 2026-11-11): each object's index, name, instance count, triangle count (its parts', modifiers
//! left out) and box (the exact box of its instances on the bed, or the raw box of an object with
//! none, which `AddDefaultInstances` never leaves); the bed (`printable_area`, `printable_height`,
//! `bed_exclude_area` as an area of three points or more, `curr_bed_type`); the printer preset's
//! name, its model and the model's id, the process and filament presets' names, the nozzle count
//! and the print sequence. Lengths are rounded to the micrometre (`round_um`), and the settings are
//! read as the C++ helpers read them ([`cfg_float`], [`cfg_string`], [`points_json`],
//! [`area_json`]). Its members are in the C++ order, and its counts are unsigned (`size_t`), as
//! nlohmann's JSON writes them.

use orca_config::OptionValue;
use orca_eigen::Vec3d;
use orca_json::{Object, Order, Value};
use orca_model::model::Model;

use crate::common::{Failure, fail, round_um};
use crate::config::Config;
use crate::presets::Settings;

/// `cfg_float(cfg, key, fallback)`: the value of a float, percent or float-or-percent setting;
/// `fallback` for one the configuration does not hold or a list.
///
/// # Errors
///
/// `internal-error` with `getFloat`'s exception for any other single value (which escapes `run`
/// in the C++).
pub fn cfg_float(cfg: &Config, key: &str, fallback: f64) -> Result<f64, Failure> {
    let Some(o) = cfg.get(key) else {
        return Ok(fallback);
    };
    if o.ty().is_vector() {
        return Ok(fallback);
    }
    match o {
        OptionValue::Float(v)
        | OptionValue::Percent(v)
        | OptionValue::FloatOrPercent { value: v, .. } => Ok(*v),
        _ => Err(fail(
            "internal-error",
            "Calling ConfigOption::getFloat on a non-float ConfigOption",
        )),
    }
}

/// `cfg_string(cfg, key)`: the setting as Orca writes it (`serialize`), "" for one the
/// configuration does not hold.
///
/// # Errors
///
/// `internal-error` with `serialize`'s exception (a NaN in a list that cannot hold one).
pub fn cfg_string(cfg: &Config, key: &str) -> Result<String, Failure> {
    match cfg.get(key) {
        None => Ok(String::new()),
        Some(o) => o.serialize().map_err(|e| fail("internal-error", e.0)),
    }
}

/// `point_json(p)`: `[x, y]`, each rounded to the micrometre.
fn point_json(p: [f64; 2]) -> Value {
    Value::Array(vec![round_um(p[0]).into(), round_um(p[1]).into()])
}

/// `point3_json(p)`: `[x, y, z]`, each rounded to the micrometre.
fn point3_json(p: &Vec3d) -> Value {
    Value::Array(vec![
        round_um(p.x()).into(),
        round_um(p.y()).into(),
        round_um(p.z()).into(),
    ])
}

/// `points_json(cfg, key)`: the points of a list-of-points setting, none when the configuration
/// holds no such setting under `key`.
#[must_use]
pub fn points_json(cfg: &Config, key: &str) -> Value {
    match cfg.get(key) {
        Some(OptionValue::Points(points)) => {
            Value::Array(points.iter().map(|p| point_json(*p)).collect())
        }
        _ => Value::Array(Vec::new()),
    }
}

/// `area_json(cfg, key)`: an area of the bed, or none. Orca keeps "no area" as one point at the
/// origin (`bed_exclude_area`'s default), and `BuildVolume` reads fewer than three points as no
/// area.
#[must_use]
pub fn area_json(cfg: &Config, key: &str) -> Value {
    match points_json(cfg, key) {
        Value::Array(points) if points.len() >= 3 => Value::Array(points),
        _ => Value::Array(Vec::new()),
    }
}

/// `info_json(model, s)`: the info job's answer.
///
/// # Errors
///
/// `internal-error` where the C++ throws: an object with no instance (`raw_bounding_box`), or a
/// setting [`cfg_float`] or [`cfg_string`] cannot read.
pub fn info_json(model: &mut Model, s: &Settings) -> Result<Value, Failure> {
    let mut objects = Vec::with_capacity(model.objects.len());
    for (i, mo) in model.objects.iter_mut().enumerate() {
        let bb = if mo.instances.is_empty() {
            mo.raw_bounding_box()
                .map_err(|e| fail("internal-error", e.0))?
        } else {
            mo.bounding_box_exact()
        };
        let mut o = Object::new(Order::Insertion);
        o.insert("objectIndex", i);
        o.insert("name", mo.name.as_str());
        o.insert("instances", mo.instances.len());
        o.insert("triangles", mo.facets_count());
        o.insert("min", point3_json(&bb.min));
        o.insert("max", point3_json(&bb.max));
        o.insert("size", point3_json(&bb.size()));
        objects.push(Value::Object(o));
    }
    let mut bed = Object::new(Order::Insertion);
    bed.insert("printableArea", points_json(&s.config, "printable_area"));
    bed.insert(
        "printableHeight",
        round_um(cfg_float(&s.config, "printable_height", 0.)?),
    );
    bed.insert("excludeArea", area_json(&s.config, "bed_exclude_area"));
    bed.insert("bedType", cfg_string(&s.config, "curr_bed_type")?);
    let filaments: Vec<Value> = s
        .filaments
        .iter()
        .map(|f| Value::from(f.name.as_str()))
        .collect();
    let mut info = Object::new(Order::Insertion);
    info.insert("objects", Value::Array(objects));
    info.insert("bed", bed);
    info.insert("printer", s.machine.name.as_str());
    info.insert("printerModel", cfg_string(&s.config, "printer_model")?);
    info.insert("printerModelId", s.printer_model_id.as_str());
    info.insert("process", s.process.name.as_str());
    info.insert("filaments", Value::Array(filaments));
    info.insert("extruders", s.extruder_count);
    info.insert("printSequence", cfg_string(&s.config, "print_sequence")?);
    Ok(Value::Object(info))
}

#[cfg(test)]
mod tests {
    use super::{area_json, cfg_float, cfg_string, points_json};
    use crate::config::Config;
    use orca_config::OptionValue;

    #[test]
    fn a_float_setting_is_read_as_getfloat_reads_it() {
        let mut c = Config::default();
        c.insert("a", OptionValue::Float(1.5));
        c.insert("b", OptionValue::Percent(20.0));
        c.insert(
            "c",
            OptionValue::FloatOrPercent {
                value: 3.0,
                percent: true,
            },
        );
        c.insert(
            "d",
            OptionValue::Floats {
                values: vec![1.0],
                nullable: false,
            },
        );
        c.insert("e", OptionValue::Int(4));
        assert_eq!(cfg_float(&c, "a", 0.), Ok(1.5));
        assert_eq!(cfg_float(&c, "b", 0.), Ok(20.0));
        assert_eq!(cfg_float(&c, "c", 0.), Ok(3.0));
        // A list, or a setting it does not hold: the fallback.
        assert_eq!(cfg_float(&c, "d", 7.), Ok(7.));
        assert_eq!(cfg_float(&c, "none", 7.), Ok(7.));
        let e = cfg_float(&c, "e", 0.).unwrap_err();
        assert_eq!(
            (e.code.as_str(), e.message.as_str()),
            (
                "internal-error",
                "Calling ConfigOption::getFloat on a non-float ConfigOption"
            )
        );
    }

    #[test]
    fn a_setting_is_written_as_orca_serializes_it_and_points_make_an_area_from_three() {
        let mut c = Config::default();
        c.insert("s", OptionValue::String("a \"b\"".to_string()));
        c.insert("one", OptionValue::Points(vec![[0.0, 0.0]]));
        c.insert(
            "three",
            OptionValue::Points(vec![[0.0, 0.0], [1.000_000_4, 0.0], [1.0, 2.5]]),
        );
        assert_eq!(cfg_string(&c, "s").unwrap(), "a \\\"b\\\"");
        assert_eq!(cfg_string(&c, "none").unwrap(), "");
        assert_eq!(points_json(&c, "one").dump(None), "[[0.0,0.0]]");
        assert_eq!(area_json(&c, "one").dump(None), "[]");
        assert_eq!(
            area_json(&c, "three").dump(None),
            "[[0.0,0.0],[1.0,0.0],[1.0,2.5]]"
        );
        // A setting that is not a list of points has no points.
        assert_eq!(points_json(&c, "s").dump(None), "[]");
    }
}
