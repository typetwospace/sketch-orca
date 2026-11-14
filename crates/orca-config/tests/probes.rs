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
// Carries no OrcaSlicer code: it replays records the C++ engine's probe wrote
// (parity/oracle/probe/layer/config.cpp). Written on 2026-11-13 (orca-o3-front-1).

//! The probe gate for orca-config's static print configs: the config records of `layer.sop1`
//! (`parity/sop1/layer.md`), from the committed subset (`parity/probes/`), or from the full set
//! when `SKETCH_ORCA_PROBES_DEEP` names its folder.
//!
//! - LYCK: each class's defaults (keys in order, each option's type and serialisation), its hash,
//!   and the `DynamicPrintConfig` made from it.
//! - LYCA: `get_abs_value` of every numeric option of the defaults, and of each float-or-percent
//!   over 0.4 and 2.5.
//! - LYCD: a copy changed by `set_deserialize_strict` (each outcome, serialisation and absolute
//!   value), then its hash, `==`, `equals`, `diff` both ways and `equal`.
//! - LYCO: `apply_only` from a dynamic config (rebuilt as the probe built it: each key cloned from
//!   `FullPrintConfig`'s defaults or the class's, then set to the recorded text), the options it
//!   leaves and the hash.
//! - LYDY: `full_print_config()` and `new_from_defaults_keys`; LYAM `get_extruder_ams_count`;
//!   LYFO `compute_filament_override_value`; LYSK `has_skirt`; LYMT `MaterialType`.
//!
//! The file's other kinds are orca-layer's (the flow, extrusion entity, surface and layer
//! records), replayed there; any kind outside both lists fails the test, as does a record count
//! other than the probe's.
//!
//! orca-config has no orca-probe dev-dependency (Cargo.toml is not this unit's to change), so the
//! reader is compiled in from its source; its own unit tests come with it.

#[allow(dead_code)]
#[path = "../../orca-probe/src/lib.rs"]
mod orca_probe;

use std::collections::BTreeMap;

use orca_config::config::Config;
use orca_config::functions::{compute_filament_override_value, get_extruder_ams_count, has_skirt};
use orca_config::material_type;
use orca_config::{
    ConfigBase, ConfigError, ConfigOption, FullPrintConfig, GCodeConfig, MachineEnvelopeConfig,
    OptionType, OptionValue, PrintConfig, PrintObjectConfig, PrintRegionConfig, SLAFullPrintConfig,
    SLAMaterialConfig, SLAPrintConfig, SLAPrintObjectConfig, SLAPrinterConfig, StaticPrintConfig,
    full_print_config, new_from_defaults_keys,
};
use orca_probe::{Mismatches, ProbeSet, Reader, load_probe};

/// The kinds this test replays, with their record counts in the committed and the full set.
const OWN: &[(&str, usize, usize)] = &[
    ("LYCK", 11, 11),
    ("LYCA", 11, 11),
    ("LYCD", 29, 260),
    ("LYCO", 29, 260),
    ("LYDY", 7, 7),
    ("LYAM", 10, 10),
    ("LYFO", 14, 14),
    ("LYSK", 10, 10),
    ("LYMT", 1, 1),
];

/// The kinds of `layer.sop1` that orca-layer replays.
const ORCA_LAYER: &[&str] = &[
    "LYFL", "LYFS", "LYFW", "LYRO", "LYEN", "LYEM", "LYEL", "LYES", "LYEC", "LYEA", "LYVW", "LYCH",
    "LYSU", "LYSC", "LYPC", "LYPR", "LYSF", "LYPH", "LYPT", "LYLA",
];

/// A recorded call: its result, or the text of the exception it threw.
#[derive(Debug, PartialEq)]
enum Outcome<T> {
    Ok(T),
    Err(String),
}

/// `attempt(p, f)`: `u8` 1 and what `f` wrote, or 0 and the exception's text.
fn read_try<'a, T>(
    r: &mut Reader<'a>,
    f: impl FnOnce(&mut Reader<'a>) -> Result<T, orca_probe::Error>,
) -> Outcome<T> {
    match r.u8().unwrap() {
        1 => Outcome::Ok(f(r).unwrap()),
        0 => Outcome::Err(r.str().unwrap()),
        v => panic!("an attempt of {v}"),
    }
}

/// The outcome of a translated call, as the probe writes one.
fn outcome<T>(got: Result<T, ConfigError>) -> Outcome<T> {
    match got {
        Ok(v) => Outcome::Ok(v),
        Err(e) => Outcome::Err(e.0),
    }
}

/// `put_abs`: `u8` 2 when the probe did not make the call, else an attempt of an `f64`.
fn read_abs(r: &mut Reader<'_>) -> Option<Outcome<u64>> {
    match r.u8().unwrap() {
        2 => None,
        1 => Some(Outcome::Ok(r.f64().unwrap().to_bits())),
        0 => Some(Outcome::Err(r.str().unwrap())),
        v => panic!("an abs value of {v}"),
    }
}

fn strs(r: &mut Reader<'_>) -> Vec<String> {
    r.vec(Reader::str).unwrap()
}

fn numeric(t: OptionType) -> bool {
    matches!(
        t,
        OptionType::Float
            | OptionType::Int
            | OptionType::Bool
            | OptionType::FloatOrPercent
            | OptionType::Percent
    )
}

fn bits(v: Result<f64, ConfigError>) -> Outcome<u64> {
    outcome(v.map(f64::to_bits))
}

fn serialize_of(c: &dyn ConfigBase, key: &str) -> Result<String, ConfigError> {
    match c.option(key) {
        Some(o) => o.serialize(),
        None => Err(ConfigError(format!("no option {key}"))),
    }
}

/// Runs `$f::<Class>(args)` for the class a record names.
macro_rules! by_class {
    ($name:expr, $f:ident, $($arg:expr),*) => {
        match $name.as_str() {
            "PrintObjectConfig" => $f::<PrintObjectConfig>($($arg),*),
            "PrintRegionConfig" => $f::<PrintRegionConfig>($($arg),*),
            "MachineEnvelopeConfig" => $f::<MachineEnvelopeConfig>($($arg),*),
            "GCodeConfig" => $f::<GCodeConfig>($($arg),*),
            "PrintConfig" => $f::<PrintConfig>($($arg),*),
            "FullPrintConfig" => $f::<FullPrintConfig>($($arg),*),
            "SLAPrintConfig" => $f::<SLAPrintConfig>($($arg),*),
            "SLAPrintObjectConfig" => $f::<SLAPrintObjectConfig>($($arg),*),
            "SLAMaterialConfig" => $f::<SLAMaterialConfig>($($arg),*),
            "SLAPrinterConfig" => $f::<SLAPrinterConfig>($($arg),*),
            "SLAFullPrintConfig" => $f::<SLAFullPrintConfig>($($arg),*),
            other => panic!("a class the probe names that has no static config here: {other}"),
        }
    };
}

fn lyck<T: StaticPrintConfig>(r: &mut Reader<'_>, m: &mut Mismatches) {
    let d = T::defaults();
    let name = T::NAME;
    let hash = r.u64().unwrap();
    m.check(d.hash() == hash, || {
        format!("LYCK {name}: hash {:#x}, the C++ {hash:#x}", d.hash())
    });
    let keys = ConfigBase::keys(d);
    let n = r.count().unwrap();
    m.check(keys.len() == n, || {
        format!("LYCK {name}: {} keys, the C++ {n}", keys.len())
    });
    for i in 0..n {
        let k = r.str().unwrap();
        let ty = r.u32().unwrap();
        let ser = r.str().unwrap();
        m.check(keys.get(i) == Some(&k), || {
            format!("LYCK {name}: key {i} {:?}, the C++ {k}", keys.get(i))
        });
        let Some(o) = d.option(&k) else {
            m.check(false, || format!("LYCK {name}: no option {k}"));
            continue;
        };
        m.check(o.opt_type().code() == ty, || {
            format!("LYCK {name}::{k}: type {:?}, the C++ {ty:#x}", o.opt_type())
        });
        let got = o.serialize();
        m.check(got.as_ref() == Ok(&ser), || {
            format!("LYCK {name}::{k}: {got:?}, the C++ {ser:?}")
        });
    }
    let dk = strs(r);
    let dynamic = d.to_dynamic();
    m.check(dynamic.keys() == dk, || {
        format!("LYCK {name}: the dynamic config's keys differ")
    });
    let same = r.bool().unwrap();
    let mut got_same = dk.len() == keys.len();
    for k in &dk {
        got_same = got_same
            && match (ConfigBase::option(&dynamic, k), d.option(k)) {
                (Some(a), Some(b)) => a.eq_opt(b) == Ok(true),
                _ => false,
            };
    }
    m.check(got_same == same, || {
        format!("LYCK {name}: same {got_same}, the C++ {same}")
    });
    r.finish("LYCK").unwrap();
}

fn lyca<T: StaticPrintConfig>(r: &mut Reader<'_>, m: &mut Mismatches) {
    let d = T::defaults();
    let name = T::NAME;
    let nk: Vec<String> = ConfigBase::keys(d)
        .into_iter()
        .filter(|k| d.option(k).is_some_and(|o| numeric(o.opt_type())))
        .collect();
    let n = r.count().unwrap();
    m.check(nk.len() == n, || {
        format!("LYCA {name}: {} numeric keys, the C++ {n}", nk.len())
    });
    for i in 0..n {
        let k = r.str().unwrap();
        m.check(nk.get(i) == Some(&k), || {
            format!("LYCA {name}: key {i} {k}")
        });
        if let Some(want) = read_abs(r) {
            let got = bits(d.get_abs_value(&k));
            m.check(got == want, || {
                format!("LYCA {name}::{k}: {got:?}, the C++ {want:?}")
            });
        }
        let is_fop = d
            .option(&k)
            .is_some_and(|o| o.opt_type() == OptionType::FloatOrPercent);
        let flag = r.u8().unwrap();
        m.check(is_fop == (flag == 1), || {
            format!("LYCA {name}::{k}: float-or-percent {is_fop}")
        });
        if flag == 1 {
            for over in [0.4, 2.5] {
                let want = r.f64().unwrap().to_bits();
                let got = bits(d.get_abs_value_over(&k, over));
                m.check(got == Outcome::Ok(want), || {
                    format!("LYCA {name}::{k} over {over}: {got:?}, the C++ {want:#x}")
                });
            }
        }
    }
    r.finish("LYCA").unwrap();
}

fn lycd<T: StaticPrintConfig>(r: &mut Reader<'_>, m: &mut Mismatches) {
    let d = T::defaults();
    let name = T::NAME;
    let ci = r.u32().unwrap();
    let mut copy = d.clone();
    let n = r.count().unwrap();
    for _ in 0..n {
        let key = r.str().unwrap();
        let value = r.str().unwrap();
        let want = read_try(r, |_| Ok(()));
        let got = outcome(copy.set_deserialize_strict(&key, &value));
        m.check(got == want, || {
            format!("LYCD {name} {ci} {key} = {value:?}: {got:?}, the C++ {want:?}")
        });
        let ser = r.str().unwrap();
        let got = serialize_of(&copy, &key);
        m.check(got.as_ref() == Ok(&ser), || {
            format!("LYCD {name} {ci} {key}: {got:?}, the C++ {ser:?}")
        });
        let is_numeric = copy.option(&key).is_some_and(|o| numeric(o.opt_type()));
        let flag = r.u8().unwrap();
        m.check(is_numeric == (flag == 1), || {
            format!("LYCD {name} {ci} {key}: numeric {is_numeric}")
        });
        if flag == 1
            && let Some(want) = read_abs(r)
        {
            let got = bits(copy.get_abs_value(&key));
            m.check(got == want, || {
                format!("LYCD {name} {ci} {key} abs: {got:?}, the C++ {want:?}")
            });
        }
    }
    let hash = r.u64().unwrap();
    m.check(copy.hash() == hash, || {
        format!(
            "LYCD {name} {ci}: hash {:#x}, the C++ {hash:#x}",
            copy.hash()
        )
    });
    let eq = r.bool().unwrap();
    m.check((copy == *d) == eq, || {
        format!("LYCD {name} {ci}: == {}", copy == *d)
    });
    let equals = r.bool().unwrap();
    let got = outcome(d.equals(&copy));
    m.check(got == Outcome::Ok(equals), || {
        format!("LYCD {name} {ci}: equals {got:?}, the C++ {equals}")
    });
    let diff = strs(r);
    let got = outcome(d.diff(&copy));
    m.check(got == Outcome::Ok(diff.clone()), || {
        format!("LYCD {name} {ci}: diff {got:?}, the C++ {diff:?}")
    });
    let equal = r.u64().unwrap();
    let got = outcome(d.equal(&copy).map(|v| v.len() as u64));
    m.check(got == Outcome::Ok(equal), || {
        format!("LYCD {name} {ci}: equal {got:?}, the C++ {equal}")
    });
    let back = strs(r);
    let got = outcome(copy.diff(d));
    m.check(got == Outcome::Ok(back.clone()), || {
        format!("LYCD {name} {ci}: diff back {got:?}, the C++ {back:?}")
    });
    r.finish("LYCD").unwrap();
}

fn lyco<T: StaticPrintConfig>(r: &mut Reader<'_>, m: &mut Mismatches) {
    let d = T::defaults();
    let name = T::NAME;
    let ci = r.u32().unwrap();
    let ignore = r.bool().unwrap();
    let list = strs(r);
    // The source: each key cloned from FullPrintConfig's defaults, else the class's, then set to
    // the text the probe recorded where the probe changed it.
    let mut src = Config::default();
    let n = r.count().unwrap();
    for _ in 0..n {
        let k = r.str().unwrap();
        let text = r.str().unwrap();
        let base = FullPrintConfig::defaults()
            .option(&k)
            .or_else(|| d.option(&k))
            .map(ConfigOption::to_value);
        let Some(mut v) = base else {
            m.check(false, || {
                format!("LYCO {name} {ci}: a source key with no option: {k}")
            });
            continue;
        };
        if v.serialize().as_ref() != Ok(&text) {
            let read = v.deserialize(&text);
            m.check(read == Ok(true), || {
                format!("LYCO {name} {ci}: the source's {k} = {text:?} reads as {read:?}")
            });
        }
        let got = v.serialize();
        m.check(got.as_ref() == Ok(&text), || {
            format!("LYCO {name} {ci}: the source's {k} is {got:?}, the C++ {text:?}")
        });
        src.insert(&k, v);
    }
    let mut copy = d.clone();
    let want = read_try(r, |_| Ok(()));
    let got = outcome(copy.apply_only(&src, &list, ignore));
    m.check(got == want, || {
        format!("LYCO {name} {ci} {list:?}: {got:?}, the C++ {want:?}")
    });
    let n = r.count().unwrap();
    m.check(n == list.len(), || {
        format!("LYCO {name} {ci}: {n} keys listed")
    });
    for _ in 0..n {
        let k = r.str().unwrap();
        let has = r.bool().unwrap();
        let value = r.str().unwrap();
        let got_has = copy.option(&k).is_some();
        m.check(got_has == has, || {
            format!("LYCO {name} {ci} {k}: has {got_has}")
        });
        if has {
            let got = serialize_of(&copy, &k);
            m.check(got.as_ref() == Ok(&value), || {
                format!("LYCO {name} {ci} {k}: {got:?}, the C++ {value:?}")
            });
        }
    }
    let hash = r.u64().unwrap();
    m.check(copy.hash() == hash, || {
        format!(
            "LYCO {name} {ci}: hash {:#x}, the C++ {hash:#x}",
            copy.hash()
        )
    });
    r.finish("LYCO").unwrap();
}

fn lydy(r: &mut Reader<'_>, m: &mut Mismatches) {
    match r.u8().unwrap() {
        0 => {
            let keys = strs(r);
            let same = r.bool().unwrap();
            let full = full_print_config();
            let d = FullPrintConfig::defaults();
            m.check(full.keys() == keys, || {
                "LYDY: full_print_config's keys".into()
            });
            let mut got_same = keys.len() == ConfigBase::keys(d).len();
            for k in &keys {
                got_same = got_same
                    && match (ConfigBase::option(&full, k), d.option(k)) {
                        (Some(a), Some(b)) => a.eq_opt(b) == Ok(true),
                        _ => false,
                    };
            }
            m.check(got_same == same, || {
                format!("LYDY: full_print_config same {got_same}, the C++ {same}")
            });
        }
        1 => {
            let list = strs(r);
            let want = read_try(r, |q| q.vec(|q| Ok((q.str()?, q.str()?))));
            let got = outcome(new_from_defaults_keys(&list).and_then(|c| {
                c.keys()
                    .into_iter()
                    .map(|k| {
                        let v = serialize_of(&c, &k)?;
                        Ok((k, v))
                    })
                    .collect::<Result<Vec<_>, ConfigError>>()
            }));
            m.check(got == want, || {
                format!("LYDY new_from_defaults_keys {list:?}: {got:?}, the C++ {want:?}")
            });
        }
        v => panic!("an LYDY of tag {v}"),
    }
    r.finish("LYDY").unwrap();
}

fn lyam(r: &mut Reader<'_>, m: &mut Mismatches) {
    let input = strs(r);
    let want = read_try(r, |q| q.vec(|q| q.vec(|q| Ok((q.i32()?, q.i32()?)))));
    let got = outcome(get_extruder_ams_count(&input).map(|maps| {
        maps.into_iter()
            .map(|mp: BTreeMap<i32, i32>| mp.into_iter().collect::<Vec<_>>())
            .collect::<Vec<_>>()
    }));
    m.check(got == want, || {
        format!("LYAM {input:?}: {got:?}, the C++ {want:?}")
    });
    r.finish("LYAM").unwrap();
}

type Overrides = (Vec<String>, Vec<(String, String)>);

fn override_case(
    key: &str,
    old: &str,
    new: &str,
    filament: &str,
    enable: i32,
    maps: &[i32],
) -> Result<Overrides, ConfigError> {
    let bools = key == "long_retractions_when_cut";
    let make = |nullable: bool| {
        if bools {
            OptionValue::Bools {
                values: Vec::new(),
                nullable,
            }
        } else {
            OptionValue::Floats {
                values: Vec::new(),
                nullable,
            }
        }
    };
    let (mut om, mut nm, mut nf) = (make(false), make(false), make(true));
    om.deserialize(old)?;
    nm.deserialize(new)?;
    nf.deserialize(filament)?;
    let mut full = Config::default();
    full.insert("enable_long_retraction_when_cut", OptionValue::Int(enable));
    let mut diff = Vec::new();
    let mut overrides = Config::default();
    compute_filament_override_value(key, &om, &nm, &nf, &full, &mut diff, &mut overrides, maps)?;
    let list = overrides
        .keys()
        .into_iter()
        .map(|k| {
            let v = serialize_of(&overrides, &k)?;
            Ok((k, v))
        })
        .collect::<Result<Vec<_>, ConfigError>>()?;
    Ok((diff, list))
}

fn lyfo(r: &mut Reader<'_>, m: &mut Mismatches) {
    let key = r.str().unwrap();
    let old = r.str().unwrap();
    let new = r.str().unwrap();
    let filament = r.str().unwrap();
    let enable = r.i32().unwrap();
    let maps = r.vec(Reader::i32).unwrap();
    let want = read_try(r, |q| {
        let diff = q.vec(Reader::str)?;
        let list = q.vec(|q| Ok((q.str()?, q.str()?)))?;
        Ok((diff, list))
    });
    let got = outcome(override_case(&key, &old, &new, &filament, enable, &maps));
    m.check(got == want, || {
        format!("LYFO {key} {old} {new} {filament} {enable} {maps:?}: {got:?}, the C++ {want:?}")
    });
    r.finish("LYFO").unwrap();
}

fn lysk(r: &mut Reader<'_>, m: &mut Mismatches) {
    let pairs = r.vec(|q| Ok((q.str()?, q.str()?))).unwrap();
    let want = read_try(r, Reader::bool);
    let got = outcome((|| {
        let mut cfg = Config::default();
        for (k, v) in &pairs {
            cfg.set_deserialize_strict(k, v)?;
        }
        has_skirt(&cfg)
    })());
    m.check(got == want, || {
        format!("LYSK {pairs:?}: {got:?}, the C++ {want:?}")
    });
    r.finish("LYSK").unwrap();
}

fn lymt(r: &mut Reader<'_>, m: &mut Mismatches) {
    let all = material_type::all();
    let n = r.count().unwrap();
    m.check(n == all.len(), || {
        format!("LYMT: {} materials, the C++ {n}", all.len())
    });
    for i in 0..n {
        let name = r.str().unwrap();
        let ints = [
            r.i32().unwrap(),
            r.i32().unwrap(),
            r.i32().unwrap(),
            r.i32().unwrap(),
        ];
        let dbls = [
            r.f64().unwrap().to_bits(),
            r.f64().unwrap().to_bits(),
            r.f64().unwrap().to_bits(),
        ];
        let same = all.get(i).is_some_and(|a| {
            a.name == name
                && [
                    a.min_temp,
                    a.max_temp,
                    a.chamber_min_temp,
                    a.chamber_max_temp,
                ] == ints
                && [
                    a.adhesion_coefficient.to_bits(),
                    a.yield_strength.to_bits(),
                    a.thermal_length.to_bits(),
                ] == dbls
        });
        m.check(same, || {
            format!("LYMT: material {i} {name}: {:?}", all.get(i))
        });
    }
    let names = r.count().unwrap();
    for _ in 0..names {
        let n = r.str().unwrap();
        let find = r.bool().unwrap();
        m.check(material_type::find(&n).is_some() == find, || {
            format!("LYMT find {n:?}")
        });
        let temp = (r.bool().unwrap(), r.i32().unwrap(), r.i32().unwrap());
        m.check(material_type::get_temperature_range(&n) == temp, || {
            format!("LYMT temperature {n:?}: the C++ {temp:?}")
        });
        let chamber = (r.bool().unwrap(), r.i32().unwrap(), r.i32().unwrap());
        m.check(
            material_type::get_chamber_temperature_range(&n) == chamber,
            || format!("LYMT chamber {n:?}: the C++ {chamber:?}"),
        );
        for (what, got) in [
            ("adhesion", material_type::get_adhesion_coefficient(&n)),
            ("yield", material_type::get_yield_strength(&n)),
            ("thermal", material_type::get_thermal_length(&n)),
        ] {
            let want = (r.bool().unwrap(), r.f64().unwrap().to_bits());
            m.check((got.0, got.1.to_bits()) == want, || {
                format!("LYMT {what} {n:?}: {got:?}, the C++ {want:?}")
            });
        }
    }
    r.finish("LYMT").unwrap();
}

#[test]
fn layer_config_probe_replays() {
    let file = load_probe("layer").unwrap_or_else(|e| panic!("{e}"));
    let full = file.set == ProbeSet::Full;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for rec in &file.records {
        *counts.entry(rec.kind().to_string()).or_default() += 1;
    }
    for kind in counts.keys() {
        assert!(
            OWN.iter().any(|(k, _, _)| k == kind) || ORCA_LAYER.contains(&kind.as_str()),
            "layer.sop1 holds {kind} records, which neither orca-config nor orca-layer replays"
        );
    }
    for (kind, committed, all) in OWN {
        let want = if full { *all } else { *committed };
        let got = counts.get(*kind).copied().unwrap_or(0);
        assert_eq!(got, want, "{kind} records in the {} set", file.set.name());
    }
    let mut m = Mismatches::default();
    for rec in &file.records {
        let mut r = rec.reader();
        match rec.kind() {
            "LYCK" => {
                let name = r.str().unwrap();
                by_class!(name, lyck, &mut r, &mut m);
            }
            "LYCA" => {
                let name = r.str().unwrap();
                by_class!(name, lyca, &mut r, &mut m);
            }
            "LYCD" => {
                let name = r.str().unwrap();
                by_class!(name, lycd, &mut r, &mut m);
            }
            "LYCO" => {
                let name = r.str().unwrap();
                by_class!(name, lyco, &mut r, &mut m);
            }
            "LYDY" => lydy(&mut r, &mut m),
            "LYAM" => lyam(&mut r, &mut m),
            "LYFO" => lyfo(&mut r, &mut m),
            "LYSK" => lysk(&mut r, &mut m),
            "LYMT" => lymt(&mut r, &mut m),
            _ => {}
        }
    }
    println!(
        "layer.sop1 ({} set): {} comparisons over {} config records",
        file.set.name(),
        m.checked,
        OWN.iter()
            .map(|(k, _, _)| counts.get(*k).copied().unwrap_or(0))
            .sum::<usize>()
    );
    m.assert_none("layer.sop1 (orca-config)");
}
