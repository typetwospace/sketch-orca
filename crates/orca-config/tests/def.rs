// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! orca-config against the configdef probe (`parity/probes/configdef.json`), which Orca's own
//! library wrote: the generated definitions hold every option and every field the probe recorded;
//! each default reads back into a value of its type and serialises to the same text; and
//! `handle_legacy` gives what Orca's gave for every key the probe tried (each legacy key and each
//! defined key, with each probe value): the cases it recorded change the key or the value as
//! recorded, and every other case changes nothing.

use std::collections::BTreeMap;
use std::path::Path;

use orca_config::{OptionType, handle_legacy, options};
use orca_json::{Order, Value, parse};

fn probe() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("parity")
        .join("probes")
        .join("configdef.json");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    parse(&bytes, Order::Insertion).expect("the probe is JSON")
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or_else(|| panic!("not a string: {v:?}"))
}

fn n(v: &Value) -> u64 {
    match v {
        Value::UInt(u) => *u,
        Value::Int(i) => u64::try_from(*i).unwrap_or_else(|_| panic!("negative: {i}")),
        _ => panic!("not an integer: {v:?}"),
    }
}

fn strings(v: &Value) -> Vec<&str> {
    v.as_array().expect("a list").iter().map(s).collect()
}

#[test]
fn the_definitions_are_the_probes() {
    let probe = probe();
    let recorded = probe["options"].as_array().expect("options");
    let defs = options();
    assert_eq!(defs.len(), recorded.len());
    assert_eq!(n(&probe["count"]), defs.len() as u64);
    for (def, o) in defs.iter().zip(recorded) {
        let key = def.key;
        assert_eq!(key, s(&o["key"]));
        assert_eq!(def.opt_key, s(&o["optKey"]), "{key}");
        assert_eq!(u64::from(def.ty.code()), n(&o["type"]), "{key}");
        assert_eq!(Some(def.nullable), o["nullable"].as_bool(), "{key}");
        assert_eq!(
            def.printer_technology as u64,
            n(&o["printerTechnology"]),
            "{key}"
        );
        for (field, got) in [
            ("category", def.category),
            ("label", def.label),
            ("fullLabel", def.full_label),
            ("tooltip", def.tooltip),
            ("sidetext", def.sidetext),
            ("cli", def.cli),
            ("cliParams", def.cli_params),
            ("ratioOver", def.ratio_over),
            ("guiFlags", def.gui_flags),
        ] {
            assert_eq!(got, s(&o[field]), "{key}.{field}");
        }
        for (field, got) in [
            ("multiline", def.multiline),
            ("fullWidth", def.full_width),
            ("isCode", def.is_code),
            ("readonly", def.readonly),
        ] {
            assert_eq!(Some(got), o[field].as_bool(), "{key}.{field}");
        }
        assert_eq!(Some(f64::from(def.height)), o["height"].as_f64(), "{key}");
        assert_eq!(Some(f64::from(def.width)), o["width"].as_f64(), "{key}");
        assert_eq!(u64::from(def.min.to_bits()), n(&o["minBits"]), "{key}");
        assert_eq!(u64::from(def.max.to_bits()), n(&o["maxBits"]), "{key}");
        assert_eq!(Some(def.max_literal), o["maxLiteral"].as_f64(), "{key}");
        assert_eq!(def.mode as u64, n(&o["mode"]), "{key}");
        assert_eq!(def.gui_type as u64, n(&o["guiType"]), "{key}");
        assert_eq!(def.aliases.to_vec(), strings(&o["aliases"]), "{key}");
        assert_eq!(def.shortcut.to_vec(), strings(&o["shortcut"]), "{key}");
        assert_eq!(def.enum_values.to_vec(), strings(&o["enumValues"]), "{key}");
        assert_eq!(def.enum_labels.to_vec(), strings(&o["enumLabels"]), "{key}");
        match (def.enum_keys_map, o["enumKeysMap"].as_object()) {
            (None, None) => {}
            (Some(map), Some(rec)) => {
                let rec: Vec<(&str, i64)> = rec
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.as_str(),
                            match v {
                                Value::Int(i) => *i,
                                Value::UInt(u) => i64::try_from(*u).expect("small"),
                                _ => panic!("{key}: not an integer"),
                            },
                        )
                    })
                    .collect();
                let got: Vec<(&str, i64)> = map.iter().map(|(k, v)| (*k, i64::from(*v))).collect();
                assert_eq!(got, rec, "{key}");
            }
            (a, b) => panic!("{key}: enum_keys_map {a:?} but the probe has {b:?}"),
        }
        assert_eq!(
            def.serialization_key_ordinal,
            n(&o["serializationKeyOrdinal"]),
            "{key}"
        );
        assert_eq!(
            def.default_type.map(|t| u64::from(t.code())),
            (!o["defaultType"].is_null()).then(|| n(&o["defaultType"])),
            "{key}"
        );
        assert_eq!(def.default, o["default"].as_str(), "{key}");
    }
    assert_eq!(
        orca_config::PRINT_OPTIONS.to_vec(),
        strings(&probe["printOptions"])
    );
    assert_eq!(
        orca_config::FILAMENT_OPTIONS.to_vec(),
        strings(&probe["filamentOptions"])
    );
    assert_eq!(
        orca_config::PRINTER_OPTIONS.to_vec(),
        strings(&probe["printerOptions"])
    );
    assert_eq!(
        orca_config::LEGACY_KEYS.to_vec(),
        strings(&probe["legacyKeys"])
    );
    assert_eq!(
        orca_config::PROBE_VALUES.to_vec(),
        strings(&probe["probeValues"])
    );
    assert!(orca_config::has("layer_height") && !orca_config::has("no_such_key"));
    assert_eq!(
        orca_config::get("layer_height").map(|d| d.ty),
        Some(OptionType::Float)
    );
}

#[test]
fn every_default_reads_back_and_serialises_to_itself() {
    let mut read = 0;
    for def in options() {
        let Some(text) = def.default else { continue };
        let value = def.default_value().expect("a default");
        assert_eq!(
            value.ty(),
            def.default_type.unwrap_or(def.ty),
            "{}",
            def.key
        );
        assert_eq!(value.serialize().as_deref(), Ok(text), "{}", def.key);
        read += 1;
    }
    assert_eq!(
        read,
        options().iter().filter(|d| d.default.is_some()).count()
    );
    assert!(read > 700, "{read} defaults");
}

#[test]
fn handle_legacy_does_what_orcas_did_for_every_case_the_probe_tried() {
    let probe = probe();
    let mut recorded: BTreeMap<(String, String), (String, String)> = BTreeMap::new();
    let cases = probe["legacy"].as_array().expect("legacy");
    for case in cases {
        let c = strings(case);
        assert_eq!(c.len(), 4);
        recorded.insert((c[0].into(), c[1].into()), (c[2].into(), c[3].into()));
    }
    let mut keys: Vec<&str> = orca_config::LEGACY_KEYS.to_vec();
    keys.extend(options().iter().map(|d| d.key));
    let mut tried = 0;
    let mut changed = 0;
    for key in keys {
        for &probe_value in orca_config::PROBE_VALUES {
            let mut k = key.to_string();
            let mut v = probe_value.to_string();
            handle_legacy(&mut k, &mut v);
            tried += 1;
            match recorded.get(&(key.to_string(), probe_value.to_string())) {
                Some((rk, rv)) => {
                    changed += 1;
                    assert_eq!(
                        (&k, &v),
                        (rk, rv),
                        "handle_legacy({key:?}, {probe_value:?})"
                    );
                }
                None => assert_eq!(
                    (k.as_str(), v.as_str()),
                    (key, probe_value),
                    "handle_legacy({key:?}, {probe_value:?}) changed nothing in Orca"
                ),
            }
        }
    }
    // The keys Orca still defines among the legacy keys were tried twice, by the probe as here.
    assert_eq!(changed, cases.len(), "every recorded case was tried");
    assert!(
        tried > 15_000 && changed > 2000,
        "{tried} tried, {changed} changed"
    );
}

#[test]
fn different_settings_to_system_renames_inside_its_list() {
    let mut k = "different_settings_to_system".to_string();
    let mut v = "\"enable_wipe_tower;layer_height\";gone_key".to_string();
    handle_legacy(&mut k, &mut v);
    assert_eq!(k, "different_settings_to_system");
    // A key Orca drops is cut out of the list; a renamed one is renamed.
    assert_eq!(v, "\"enable_prime_tower;layer_height\";");
}
