// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-placeholder: the records of `placeholder.sop1` (`parity/SOP1.md`, "Probe
//! files"), from the committed subset (`parity/probes/`), or from the full set when
//! `SKETCH_ORCA_PROBES_DEEP` names its folder (`<work>/parity/probes`).
//!
//! Each PHEV record is one `evaluate_boolean_expression` of the C++ engine's build, against the
//! configuration (and override) of its PHCF records, rebuilt here from their serialized values.
//! Its outcome must be this crate's: the same result, the same `PlaceholderParserError` text, and
//! a decline (`Error::Unsupported`) exactly where the C++ code threw another exception (an error
//! at a null iterator). Replays added 2026-11-07 (o2-front).

use std::collections::BTreeMap;

use orca_config::OptionValue;
use orca_placeholder::{Error, evaluate_boolean_expression};
use orca_probe::{Mismatches, load_probe};

type Config = BTreeMap<String, OptionValue>;

/// A configuration of a PHCF record: each key's value read from its serialized text, as the
/// option its definition makes (the override's `printer_preset` and `num_extruders`, which no
/// definition has, as a string and an int).
fn config_of(r: &mut orca_probe::Reader<'_>) -> (u32, Config) {
    let id = r.u32().unwrap();
    let _name = r.bytes().unwrap();
    let n = r.count().unwrap();
    let mut c = Config::new();
    for _ in 0..n {
        let key = r.str().unwrap();
        let text = r.str().unwrap();
        let value = match orca_config::get(&key) {
            Some(def) => {
                let mut v = def.empty_value();
                v.deserialize(&text)
                    .unwrap_or_else(|e| panic!("{key} = {text}: {e:?}"));
                v
            }
            None if key == "num_extruders" => OptionValue::Int(text.parse().unwrap()),
            None => OptionValue::String(
                orca_config::value::unescape_string_cstyle(&text).unwrap_or(text),
            ),
        };
        c.insert(key, value);
    }
    (id, c)
}

#[test]
fn placeholder_probe_replays() {
    let file = load_probe("placeholder").unwrap_or_else(|e| panic!("{e}"));
    let mut configs: BTreeMap<u32, Config> = BTreeMap::new();
    for rec in file.of_kind("PHCF") {
        let (id, c) = config_of(&mut rec.reader());
        configs.insert(id, c);
    }
    let mut m = Mismatches::default();
    let mut outcomes = [0u64; 4];
    for rec in file.of_kind("PHEV") {
        let mut r = rec.reader();
        let config_id = r.u32().unwrap();
        let override_id = r.i32().unwrap();
        let templ = r.bytes().unwrap().to_vec();
        let outcome = r.u8().unwrap();
        let result = r.bool().unwrap();
        let message = String::from_utf8_lossy(r.bytes().unwrap()).into_owned();
        r.finish("PHEV").unwrap();
        outcomes[usize::from(outcome.min(3))] += 1;
        let config = &configs[&config_id];
        let over = (override_id >= 0).then(|| &configs[&(override_id as u32)]);
        let got = evaluate_boolean_expression(
            &templ,
            config,
            over.map(|o| o as &dyn orca_placeholder::Symbols),
        );
        let same = match (outcome, &got) {
            (0, Ok(b)) => *b == result,
            (1, Err(Error::Parse(msg))) => *msg == message,
            (2 | 3, Err(Error::Unsupported(_))) => true,
            _ => false,
        };
        m.check(same, || {
            format!(
                "{:?} (config {config_id}, override {override_id}): C++ outcome {outcome} {result} {message:?}, here {got:?}",
                String::from_utf8_lossy(&templ)
            )
        });
    }
    eprintln!(
        "{}: {} evaluations ({} results, {} parse errors, {} other exceptions)",
        file.path.display(),
        m.checked,
        outcomes[0],
        outcomes[1],
        outcomes[2] + outcomes[3]
    );
    m.assert_none("placeholder.sop1 PHEV");
}
