// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-slice: the records of `slice.sop1` (`parity/sop1/slice.md`) that the
//! piece slice-1 owns, replayed against the crate bit for bit (added 2026-11-13 by slice-1):
//!
//! - SLPR: `SlicingParameters::create_from_config` on the record's settings and again with one
//!   more setting, `equal_layering` of the two, and `Slicing::min_layer_height_from_nozzle` and
//!   `max_layer_height_from_nozzle` for every index the record holds, on a `DynamicPrintConfig`
//!   of the settings' three nozzle options;
//! - SLRG: `layer_height_profile_from_ranges` on the recorded parameters and ranges.
//!
//! slice-1's lines of `TriangleMeshSlicer.cpp` have no record of their own: the engine reaches
//! them only through `slice_mesh`, `slice_mesh_ex` and `slice_mesh_slabs`, so slice-2's SLMS,
//! SLM1, SLMX and SLSB hold them. Every `f64` is compared by its bits (a sign of zero or a NaN
//! counts). slice-2 (2026-11-14) replays its nine kinds in `probes/slice_2.rs`; front-2 replays
//! its own in orca-model, and this file checks that no kind outside the three pieces is in the
//! file.
//!
//! The committed subset (`parity/probes/`) runs by default; `SKETCH_ORCA_PROBES_DEEP=<folder>`
//! replays the full set from that folder (`<work>/parity/probes`).

#[path = "probes/slice_2.rs"]
mod slice_2;

use std::fmt::{self, Debug, Formatter};

use orca_config::config::Config;
use orca_config::{ConfigBase, OptionValue, PrintConfig, PrintObjectConfig, StaticPrintConfig};
use orca_eigen::Vector;
use orca_model::config::ModelConfig;
use orca_model::model::LayerConfigRanges;
use orca_probe::{Mismatches, ProbeFile, ProbeSet, Reader, load_probe};
use orca_slice::slicing::{
    SlicingParameters, equal_layering, layer_height_profile_from_ranges,
    max_layer_height_from_nozzle, min_layer_height_from_nozzle,
};

fn file() -> ProbeFile {
    load_probe("slice").unwrap_or_else(|e| panic!("{e}"))
}

/// The kinds this file replays.
const SLICE_1_KINDS: [&str; 2] = ["SLPR", "SLRG"];
/// The kinds front-2 replays, and SLMH, the mesh table its records and slice-2's name.
const OTHER_KINDS: [&str; 6] = ["SLMH", "SLFE", "SLMC", "SLTS", "SLSH", "SLBX"];

/// A field of the parameters as the record writes it: a `bool`, a `u64` count or an `f64`, the
/// last compared by its bits.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Bool(bool),
    Count(u64),
    Bits(u64),
}

impl Debug for Field {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(v) => write!(f, "{v}"),
            Self::Count(v) => write!(f, "{v}"),
            Self::Bits(v) => write!(f, "{:?}", f64::from_bits(*v)),
        }
    }
}

/// The names of the parameters' fields, in the order the record writes them.
const FIELD_NAMES: [&str; 26] = [
    "valid",
    "base_raft_layers",
    "interface_raft_layers",
    "base_raft_layer_height",
    "interface_raft_layer_height",
    "contact_raft_layer_height",
    "layer_height",
    "min_layer_height",
    "max_layer_height",
    "max_suport_layer_height",
    "first_print_layer_height",
    "first_object_layer_height",
    "first_object_layer_bridging",
    "zero_gap_interface_top",
    "zero_gap_interface_bottom",
    "zero_gap_interface_raft",
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
];

fn count(n: usize) -> Field {
    Field::Count(u64::try_from(n).unwrap())
}

/// The parameters' fields in the record's order.
fn fields(p: &SlicingParameters) -> [Field; 26] {
    let b = |v: f64| Field::Bits(v.to_bits());
    [
        Field::Bool(p.valid),
        count(p.base_raft_layers),
        count(p.interface_raft_layers),
        b(p.base_raft_layer_height),
        b(p.interface_raft_layer_height),
        b(p.contact_raft_layer_height),
        b(p.layer_height),
        b(p.min_layer_height),
        b(p.max_layer_height),
        b(p.max_suport_layer_height),
        b(p.first_print_layer_height),
        b(p.first_object_layer_height),
        Field::Bool(p.first_object_layer_bridging),
        Field::Bool(p.zero_gap_interface_top),
        Field::Bool(p.zero_gap_interface_bottom),
        Field::Bool(p.zero_gap_interface_raft),
        b(p.gap_raft_object),
        b(p.gap_object_support),
        b(p.gap_support_object),
        b(p.raft_base_top_z),
        b(p.raft_interface_top_z),
        b(p.raft_contact_top_z),
        b(p.object_print_z_min),
        b(p.object_print_z_max),
        b(p.object_print_z_uncompensated_max),
        b(p.object_shrinkage_compensation_z),
    ]
}

/// One record's reader, the mismatch list and the record's label.
struct Case<'a, 'm> {
    r: Reader<'a>,
    m: &'m mut Mismatches,
    label: String,
}

impl Case<'_, '_> {
    fn u32(&mut self) -> u32 {
        self.r.u32().unwrap()
    }
    fn u64(&mut self) -> u64 {
        self.r.u64().unwrap()
    }
    fn f64(&mut self) -> f64 {
        self.r.f64().unwrap()
    }
    fn bool(&mut self) -> bool {
        self.r.bool().unwrap()
    }
    fn str(&mut self) -> String {
        self.r.str().unwrap()
    }
    /// A `u64` count.
    fn index(&mut self) -> usize {
        usize::try_from(self.u64()).unwrap()
    }
    /// The settings: a count, each key and value.
    fn settings(&mut self) -> Vec<(String, String)> {
        let n = self.index();
        (0..n)
            .map(|_| {
                let key = self.str();
                (key, self.str())
            })
            .collect()
    }
    /// The parameters, in the order of `put_params`.
    fn params(&mut self) -> SlicingParameters {
        SlicingParameters {
            valid: self.bool(),
            base_raft_layers: self.index(),
            interface_raft_layers: self.index(),
            base_raft_layer_height: self.f64(),
            interface_raft_layer_height: self.f64(),
            contact_raft_layer_height: self.f64(),
            layer_height: self.f64(),
            min_layer_height: self.f64(),
            max_layer_height: self.f64(),
            max_suport_layer_height: self.f64(),
            first_print_layer_height: self.f64(),
            first_object_layer_height: self.f64(),
            first_object_layer_bridging: self.bool(),
            zero_gap_interface_top: self.bool(),
            zero_gap_interface_bottom: self.bool(),
            zero_gap_interface_raft: self.bool(),
            gap_raft_object: self.f64(),
            gap_object_support: self.f64(),
            gap_support_object: self.f64(),
            raft_base_top_z: self.f64(),
            raft_interface_top_z: self.f64(),
            raft_contact_top_z: self.f64(),
            object_print_z_min: self.f64(),
            object_print_z_max: self.f64(),
            object_print_z_uncompensated_max: self.f64(),
            object_shrinkage_compensation_z: self.f64(),
        }
    }
    /// The ranges: a count, each low z, high z and the range's `layer_height`, in the map's
    /// order.
    fn ranges(&mut self) -> LayerConfigRanges {
        let n = self.index();
        (0..n)
            .map(|_| {
                let lo = self.f64();
                let hi = self.f64();
                let mut config = ModelConfig::new();
                config.set_key_value("layer_height", OptionValue::Float(self.f64()));
                ((lo, hi), config)
            })
            .collect()
    }
    /// A profile: a count of `f64`.
    fn profile(&mut self) -> Vec<f64> {
        let n = self.index();
        (0..n).map(|_| self.f64()).collect()
    }

    fn same<T: PartialEq + Debug>(&mut self, what: &str, got: &T, want: &T) {
        let label = &self.label;
        self.m.check(got == want, || {
            format!("{label}: {what}: {got:?} instead of {want:?}")
        });
    }
    fn is_bool(&mut self, what: &str, got: bool) {
        let want = self.bool();
        self.same(what, &got, &want);
    }
    fn is_f64(&mut self, what: &str, got: f64) {
        let want = Field::Bits(self.f64().to_bits());
        self.same(what, &Field::Bits(got.to_bits()), &want);
    }
    /// The recorded parameters against `got`, field by field (the first that differs named).
    fn is_params(&mut self, what: &str, got: &SlicingParameters) {
        let want = fields(&self.params());
        let got = fields(got);
        match (0..want.len()).find(|&k| got[k] != want[k]) {
            Some(k) => self.same(&format!("{what}.{}", FIELD_NAMES[k]), &got[k], &want[k]),
            None => self.m.check(true, String::new),
        }
    }
    /// A profile against `got`: its count, then the first value that differs.
    fn is_profile(&mut self, what: &str, got: &[f64]) {
        let want: Vec<Field> = self
            .profile()
            .iter()
            .map(|v| Field::Bits(v.to_bits()))
            .collect();
        let got: Vec<Field> = got.iter().map(|v| Field::Bits(v.to_bits())).collect();
        if got.len() != want.len() {
            self.same(what, &got, &want);
        } else if let Some(k) = (0..got.len()).find(|&k| got[k] != want[k]) {
            self.same(&format!("{what} [{k}]"), &got[k], &want[k]);
        } else {
            self.m.check(true, String::new);
        }
    }
}

/// Replays every record of `kind`; the full set must hold `full_count` of them
/// (`parity/sop1/slice.md`), so a replay that reads fewer fails.
fn replay(kind: &str, full_count: usize, f: impl FnMut(&mut Case<'_, '_>)) {
    replay_in(&file(), kind, full_count, f);
}

/// [`replay`] on a file already loaded.
fn replay_in(
    file: &ProbeFile,
    kind: &str,
    full_count: usize,
    mut f: impl FnMut(&mut Case<'_, '_>),
) {
    let mut m = Mismatches::default();
    let mut records = 0;
    for (i, rec) in file.of_kind(kind).enumerate() {
        let before = m.count;
        let mut c = Case {
            r: rec.reader(),
            m: &mut m,
            label: format!("{kind} {i}"),
        };
        f(&mut c);
        if c.m.count == before {
            c.r.finish(&c.label).unwrap();
        }
        records += 1;
    }
    m.assert_none(kind);
    assert!(records > 0, "{}: no {kind} record", file.path.display());
    if file.set == ProbeSet::Full {
        assert_eq!(
            records,
            full_count,
            "{}: {kind} records",
            file.path.display()
        );
    }
}

/// The probe's `apply_settings`: a `PrintConfig` and a `PrintObjectConfig` at their defaults,
/// each setting set from its text on whichever of the two holds its key (both when both do).
fn configs(settings: &[(String, String)]) -> (PrintConfig, PrintObjectConfig) {
    let mut pc = PrintConfig::default();
    let mut oc = PrintObjectConfig::default();
    for (key, value) in settings {
        let mut any = false;
        if pc.optptr(key).is_some() {
            pc.set_deserialize_strict(key, value).unwrap();
            any = true;
        }
        if oc.optptr(key).is_some() {
            oc.set_deserialize_strict(key, value).unwrap();
            any = true;
        }
        assert!(any, "no config holds the key {key}");
    }
    (pc, oc)
}

#[test]
fn slpr_parameters_and_nozzle_limits() {
    replay("SLPR", 160, |c| {
        let settings = c.settings();
        let height = c.f64();
        let n = c.index();
        let extruders: Vec<u32> = (0..n).map(|_| c.u32()).collect();
        let shrink = Vector([c.f64(), c.f64(), c.f64()]);
        let (pc, oc) = configs(&settings);
        let a = SlicingParameters::create_from_config(&pc, &oc, height, &extruders, &shrink);
        c.is_params("a", &a);

        let key = c.str();
        let value = c.str();
        let mut variant = settings.clone();
        variant.push((key, value));
        let (pc, oc) = configs(&variant);
        let b = SlicingParameters::create_from_config(&pc, &oc, height, &extruders, &shrink);
        c.is_params("b", &b);
        c.is_bool("equal_layering", equal_layering(&a, &b));

        let mut dynamic = Config::default();
        for (key, value) in &settings {
            if key == "nozzle_diameter" || key == "min_layer_height" || key == "max_layer_height" {
                dynamic.set_deserialize_strict(key, value).unwrap();
            }
        }
        let indices = c.index();
        for idx in 0..indices {
            let idx = i32::try_from(idx).unwrap();
            c.is_f64(
                &format!("min_layer_height_from_nozzle({idx})"),
                min_layer_height_from_nozzle(&dynamic, idx),
            );
            c.is_f64(
                &format!("max_layer_height_from_nozzle({idx})"),
                max_layer_height_from_nozzle(&dynamic, idx),
            );
        }
    });
}

#[test]
fn slrg_profile_from_ranges() {
    replay("SLRG", 100, |c| {
        let params = c.params();
        let ranges = c.ranges();
        let profile = layer_height_profile_from_ranges(&params, &ranges);
        c.is_profile("profile", &profile);
    });
}

/// Every kind of the file is slice-1's, slice-2's or another piece's, and every kind of the two
/// is recorded.
#[test]
fn every_kind_is_known() {
    let file = file();
    let unknown: Vec<&str> = file
        .records
        .iter()
        .map(|r| r.kind())
        .filter(|k| {
            !SLICE_1_KINDS.contains(k)
                && !slice_2::SLICE_2_KINDS.contains(k)
                && !OTHER_KINDS.contains(k)
        })
        .collect();
    assert!(
        unknown.is_empty(),
        "{}: {} records nothing replays ({:?}...)",
        file.path.display(),
        unknown.len(),
        unknown.iter().take(4).collect::<Vec<_>>()
    );
    for kind in SLICE_1_KINDS.iter().chain(&slice_2::SLICE_2_KINDS) {
        assert!(
            file.of_kind(kind).next().is_some(),
            "{}: no {kind} record",
            file.path.display()
        );
    }
}
