// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: class ModelConfig of src/libslic3r/PrintConfig.hpp and
// ConfigBase::set_deserialize, set_deserialize_nothrow and set_deserialize_raw of
// src/libslic3r/Config.cpp with ConfigOptionBoolsTempl::deserialize_with_substitutions and
// ConfigHelpers of Config.hpp, of OrcaSlicer v2.4.2, translated on 2026-11-07. Copyright the
// OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa
// Research), under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `ModelConfig`: the settings of an object, a part or a height range, a `DynamicPrintConfig`
//! whose options are kept by key in key order, as its `std::map` keeps them.
//!
//! The readers set options from text with [`ModelConfig::set_deserialize`] under the rule
//! `load_model` passes (`ForwardCompatibilitySubstitutionRule::EnableSilent`): the legacy rename
//! or drop of `handle_legacy`, an alias, and an enum, a bool or a list of bools Orca does not know
//! substituted without a record (the substitutions are never written by the front end). Orca's
//! exceptions come back as [`SetError`], with the C++ `what()` text.
//!
//! The same translation of `set_deserialize_raw` is in orca-cli's `config.rs`; the two change
//! together (orca-model cannot depend on orca-cli, which links it).

use std::collections::BTreeMap;

use orca_config::{OptionDef, OptionType, OptionValue};

/// The defaults of Orca's nine `coEnums` options (`ConfigOptionEnumsGeneric{...}` in
/// PrintConfig.cpp), which orca-config's definitions record as "" (as orca-cli's `config.rs`).
const ENUMS_DEFAULTS: &[(&str, i32)] = &[
    ("default_nozzle_volume_type", 0),
    ("extruder_type", 0),
    ("filament_retract_lift_enforce", 0),
    ("filament_z_hop_types", 2),
    ("nozzle_type", 0),
    ("nozzle_volume_type", 0),
    ("overhang_fan_threshold", 5),
    ("retract_lift_enforce", 0),
    ("z_hop_types", 2),
];

/// What `ConfigBase::set_deserialize` throws, with the C++ `what()` text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetError {
    /// `UnknownOptionException(key)`: "Unknown option exception: <key>".
    Unknown(String),
    /// `BadOptionValueException`: "Invalid value provided for parameter <key>: <value>".
    BadValue(String),
    /// `ConfigurationError` of a value's deserialisation ("Deserializing nil into a non-nullable
    /// object").
    Config(String),
}

impl SetError {
    /// The exception's `what()`.
    #[must_use]
    pub fn what(&self) -> String {
        match self {
            SetError::Unknown(key) => format!("Unknown option exception: {key}"),
            SetError::BadValue(m) | SetError::Config(m) => m.clone(),
        }
    }
}

/// `def->default_value`: the option's default, or `None` when it has none.
fn default_value(def: &OptionDef) -> Option<OptionValue> {
    if def.ty == OptionType::Enums
        && let Some((_, v)) = ENUMS_DEFAULTS.iter().find(|(k, _)| *k == def.key)
    {
        return Some(OptionValue::Enums {
            values: vec![*v],
            keys: def.enum_keys_map,
            nullable: def.nullable,
        });
    }
    def.default_value()
}

/// `create_default_option()`.
fn create_default(def: &OptionDef) -> OptionValue {
    default_value(def).unwrap_or_else(|| def.empty_value())
}

/// `print_config_def.get(key)`, else the option that has `key` as an alias.
fn find_def(key: &str) -> Option<&'static OptionDef> {
    orca_config::get(key).or_else(|| {
        orca_config::options()
            .iter()
            .find(|d| d.aliases.contains(&key))
    })
}

/// The classic locale's `isspace`.
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// `boost::trim` with the classic locale's white space.
fn trim(s: &[u8]) -> &[u8] {
    let start = s.iter().position(|&c| !is_space(c)).unwrap_or(s.len());
    let end = s
        .iter()
        .rposition(|&c| !is_space(c))
        .map_or(start, |i| i + 1);
    &s[start..end]
}

/// The pieces `while (std::getline(is, item, delim))` reads.
fn getline_split(s: &[u8], delim: u8) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < s.len() {
        match s[pos..].iter().position(|&c| c == delim) {
            Some(i) => {
                out.push(&s[pos..pos + i]);
                pos += i + 1;
            }
            None => {
                out.push(&s[pos..]);
                pos = s.len();
            }
        }
    }
    out
}

/// `ConfigHelpers::looks_like_enum_value`.
fn looks_like_enum_value(value: &[u8]) -> bool {
    let v = trim(value);
    !v.is_empty()
        && v.len() <= 64
        && v[0].is_ascii_alphabetic()
        && v.iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')
}

/// `ConfigHelpers::enum_looks_like_true_value`.
fn enum_looks_like_true_value(value: &[u8]) -> bool {
    let v = trim(value);
    v.eq_ignore_ascii_case(b"enabled") || v.eq_ignore_ascii_case(b"on")
}

/// `ConfigHelpers::DeserializationResult`.
enum BoolsRead {
    Loaded,
    Substituted,
    Failed,
}

/// `ConfigOptionBoolsTempl::deserialize_with_substitutions`.
fn bools_with_substitutions(
    values: &mut Vec<u8>,
    nullable: bool,
    text: &str,
    defaults_to_true: bool,
) -> Result<BoolsRead, SetError> {
    values.clear();
    let mut substituted = false;
    for item in getline_split(text.as_bytes(), b',') {
        let item = trim(item);
        let v = if item == b"nil" {
            if !nullable {
                return Err(SetError::Config(
                    "Deserializing nil into a non-nullable object".into(),
                ));
            }
            u8::MAX
        } else if item == b"1" {
            1
        } else if item == b"0" {
            0
        } else if looks_like_enum_value(item) {
            substituted = true;
            u8::from(enum_looks_like_true_value(item) || defaults_to_true)
        } else {
            return Ok(BoolsRead::Failed);
        };
        values.push(v);
    }
    Ok(if substituted {
        BoolsRead::Substituted
    } else {
        BoolsRead::Loaded
    })
}

/// `ModelConfig` (and `ModelConfigObject`, whose id and timestamp the front end never reads).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelConfig {
    options: BTreeMap<String, OptionValue>,
}

impl ModelConfig {
    /// An empty configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `has(key)`.
    #[must_use]
    pub fn has(&self, key: &str) -> bool {
        self.options.contains_key(key)
    }

    /// `option(key)`.
    #[must_use]
    pub fn option(&self, key: &str) -> Option<&OptionValue> {
        self.options.get(key)
    }

    /// `keys()`, in key order.
    #[must_use]
    pub fn keys(&self) -> Vec<String> {
        self.options.keys().cloned().collect()
    }

    /// Every option, in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &OptionValue)> {
        self.options.iter()
    }

    /// `empty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.options.is_empty()
    }

    /// `set_key_value(key, value)`.
    pub fn set_key_value(&mut self, key: &str, value: OptionValue) {
        self.options.insert(key.to_string(), value);
    }

    /// `erase(key)`.
    pub fn erase(&mut self, key: &str) -> bool {
        self.options.remove(key).is_some()
    }

    /// `reset()`.
    pub fn reset(&mut self) {
        self.options.clear();
    }

    /// `extruder()`: `opt_int("extruder")`, the option that must be there and an int.
    #[must_use]
    pub fn extruder(&self) -> i32 {
        match self.options.get("extruder") {
            Some(OptionValue::Int(v)) => *v,
            _ => panic!("ModelConfig::extruder() of a configuration without an int extruder"),
        }
    }

    /// `dynamic_cast<const ConfigOptionInt*>(option(key))`.
    #[must_use]
    pub fn opt_int(&self, key: &str) -> Option<i32> {
        match self.options.get(key) {
            Some(OptionValue::Int(v)) => Some(*v),
            _ => None,
        }
    }

    /// `dynamic_cast<const ConfigOptionBool*>(option(key))`.
    #[must_use]
    pub fn opt_bool(&self, key: &str) -> Option<bool> {
        match self.options.get(key) {
            Some(OptionValue::Bool(v)) => Some(*v),
            _ => None,
        }
    }

    /// `set_deserialize(key, value, substitutions)` under `EnableSilent`.
    ///
    /// # Errors
    ///
    /// [`SetError::Unknown`] for a key no option has, [`SetError::BadValue`] for a value the
    /// option does not read, [`SetError::Config`] for "nil" in an option that takes none.
    pub fn set_deserialize(&mut self, key: &str, value: &str) -> Result<(), SetError> {
        let (mut k, mut v) = (key.to_string(), value.to_string());
        orca_config::handle_legacy(&mut k, &mut v);
        if k.is_empty() {
            return Ok(());
        }
        if self.set_deserialize_raw(&k, &v)? {
            Ok(())
        } else {
            Err(SetError::BadValue(format!(
                "Invalid value provided for parameter {key}: {value}"
            )))
        }
    }

    /// `set_deserialize_raw(key, value, ctx)` under `EnableSilent`.
    fn set_deserialize_raw(&mut self, key: &str, value: &str) -> Result<bool, SetError> {
        let Some(def) = find_def(key) else {
            return Err(SetError::Unknown(key.to_string()));
        };
        if !def.shortcut.is_empty() {
            for shortcut in def.shortcut {
                if !self.set_deserialize_raw(shortcut, value)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        let opt = self
            .options
            .entry(def.key.to_string())
            .or_insert_with(|| create_default(def));
        if def.ty == OptionType::Bools {
            let defaults_to_true = matches!(
                default_value(def),
                Some(OptionValue::Bools { values, .. }) if values.len() == 1 && values[0] == 1
            );
            let OptionValue::Bools { values, nullable } = opt else {
                unreachable!("a coBools option holds bools");
            };
            return Ok(
                match bools_with_substitutions(values, *nullable, value, defaults_to_true)? {
                    BoolsRead::Loaded | BoolsRead::Substituted => true,
                    BoolsRead::Failed => false,
                },
            );
        }
        let read = opt
            .deserialize(value)
            .map_err(|e| SetError::Config(e.0.clone()))?;
        if !read
            && matches!(
                def.ty,
                OptionType::Enum | OptionType::Enums | OptionType::Bool
            )
        {
            if def.ty == OptionType::Bool {
                *opt = OptionValue::Bool(enum_looks_like_true_value(value.as_bytes()));
            } else {
                *opt = create_default(def);
            }
            return Ok(true);
        }
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extruder_reads_and_unknown_keys_are_ignored() {
        let mut c = ModelConfig::new();
        c.set_deserialize("extruder", "2").unwrap();
        assert_eq!(c.extruder(), 2);
        // handle_legacy clears a key the engine does not know, so it is ignored.
        c.set_deserialize("no_such_key", "1").unwrap();
        assert!(!c.has("no_such_key"));
        assert_eq!(
            c.set_deserialize("extruder", "x").unwrap_err().what(),
            "Invalid value provided for parameter extruder: x"
        );
    }
}
