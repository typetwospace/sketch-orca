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
// Carries OrcaSlicer code: StaticPrintConfig and StaticCache of src/libslic3r/PrintConfig.hpp
// (lines 557-916: the option table by key, keys in the definition's order, the defaults from the
// definition, optptr, keys_ref and defaults), ConfigBase::apply_only, equals, diff, equal,
// opt_serialize, set(key, int) and set(key, double), set_deserialize_strict through
// set_deserialize_raw, and both get_abs_value of src/libslic3r/Config.cpp (lines 461-571 and
// 690-754), StaticConfig::set_defaults and keys and DynamicConfig's equals, diff and equal
// (Config.cpp lines 1798-1887), DynamicConfig(const ConfigBase&, keys) (Config.cpp lines
// 1530-1534), and DynamicPrintConfig::full_print_config, DynamicPrintConfig(const
// StaticPrintConfig&) and new_from_defaults_keys of src/libslic3r/PrintConfig.cpp (lines
// 8477-8491), all of OrcaSlicer v2.4.2, commit 8500fcd, translated on 2026-11-13
// (orca-o3-front-1); their copyright is the OrcaSlicer contributors', under the AGPL-3.0.

//! What a static print config does ([`StaticPrintConfig`]) and what any config does
//! ([`ConfigBase`]), for the classes of [`crate::print_config`] and the dynamic
//! [`Config`](crate::config::Config).
//!
//! - A class's [`StaticCache`] is made once: its keys are the definition's keys it holds, in the
//!   definition's order (by key), and its defaults are the class as constructed with each of
//!   those options set from the definition's default ([`default_value`], which keeps the defaults
//!   a serialisation loses). [`StaticPrintConfig::defaults`] and `Default` give that copy.
//! - [`StaticPrintConfig::optptr`] finds a field by key through the class's table (`BY_KEY`,
//!   sorted by key); it holds every field, a field the definition lacks included.
//! - [`ConfigBase`] is the C++'s ConfigBase over `option`, `option(key, true)` and `keys`:
//!   `apply_only` (with its `key#index` form), `apply`, `equals`, `diff` and `equal` (keys of
//!   this config, skipping a key either side lacks, compared with the virtual `operator==`),
//!   `opt_serialize`, `get_abs_value` (a percent over its `ratio_over` chain),
//!   `set_deserialize_strict` and the two `set`s. A static class's `option(key, true)` makes
//!   nothing; the dynamic config's makes a defined key with its default.
//! - [`full_print_config`] and [`StaticPrintConfig::to_dynamic`] copy a static class into a
//!   [`Config`]; [`new_from_defaults_keys`] makes one holding some keys at `FullPrintConfig`'s
//!   defaults.
//!
//! Where the C++ leaves defined behaviour (a null option dereferenced by `apply_only`,
//! `set_deserialize_raw` or `get_abs_value`, a percent cast to a float-or-percent, a key with no
//! definition), these return a [`ConfigError`] that says so; the probes never reach those cases
//! (parity/HAZARDS.md).

use std::fmt::Debug;

use crate::config::{Config, default_value, find_def};
use crate::option::ConfigOption;
use crate::print_config::FullPrintConfig;
use crate::{ConfigError, OptionType, OptionValue};

/// `StaticCache<T>`: the keys of a class and its defaults.
#[derive(Debug)]
pub struct StaticCache<T> {
    keys: Vec<&'static str>,
    defaults: T,
}

impl<T: StaticPrintConfig> StaticCache<T> {
    /// `T::initialize_cache()`: the keys in the definition's order and the defaults.
    ///
    /// # Panics
    ///
    /// If a definition's default does not set its option, which `tests/probes.rs` rules out.
    #[must_use]
    pub fn new() -> Self {
        let mut defaults = T::constructed();
        let keys: Vec<&'static str> = crate::options()
            .iter()
            .filter(|def| T::index_of(def.key).is_some())
            .map(|def| def.key)
            .collect();
        // StaticConfig::set_defaults.
        for key in &keys {
            let (Some(def), Some(i)) = (crate::get(key), T::index_of(key)) else {
                continue;
            };
            if let Some(value) = default_value(def)
                && let Err(e) = defaults.opt_mut(i).set_value(&value)
            {
                panic!("{}::{key}: its default does not set: {}", T::NAME, e.0);
            }
        }
        Self { keys, defaults }
    }
}

impl<T: StaticPrintConfig> Default for StaticCache<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// A static print config class (`StaticPrintConfig` with the `PRINT_CONFIG_CLASS_*` macros).
pub trait StaticPrintConfig: Clone + PartialEq + Debug + Sized + 'static {
    /// The class's name.
    const NAME: &'static str;
    /// Each field's key, in field order (a parent's fields first, as the C++ lays them out).
    const FIELDS: &'static [&'static str];
    /// Each field's key with its index in [`Self::FIELDS`], sorted by key (byte order).
    const BY_KEY: &'static [(&'static str, u16)];

    /// The class as its constructor leaves it before `initialize`: each option as its type's
    /// constructor makes it.
    fn constructed() -> Self;
    /// The class's cache.
    fn cache() -> &'static StaticCache<Self>;
    /// Field `i` (of [`Self::FIELDS`]).
    fn opt(&self, i: usize) -> &dyn ConfigOption;
    /// Field `i`, to change it.
    fn opt_mut(&mut self, i: usize) -> &mut dyn ConfigOption;
    /// `hash()`: the parents' hashes, then each field's, combined from 0.
    fn hash(&self) -> u64;

    /// `defaults()`.
    #[must_use]
    fn defaults() -> &'static Self {
        &Self::cache().defaults
    }

    /// `keys_ref()`: the keys the definition has, in its order.
    #[must_use]
    fn keys_ref() -> &'static [&'static str] {
        &Self::cache().keys
    }

    /// The field index of a key.
    #[must_use]
    fn index_of(key: &str) -> Option<usize> {
        let all = Self::BY_KEY;
        let i = orca_stl::lower_bound_by(all, key, |e, key: &str| e.0.as_bytes() < key.as_bytes());
        all.get(i).filter(|e| e.0 == key).map(|e| usize::from(e.1))
    }

    /// `optptr(key)`: the field, or `None` for a key the class lacks.
    #[must_use]
    fn optptr(&self, key: &str) -> Option<&dyn ConfigOption> {
        Self::index_of(key).map(|i| self.opt(i))
    }

    /// `optptr(key)`, to change it.
    fn optptr_mut(&mut self, key: &str) -> Option<&mut dyn ConfigOption> {
        Self::index_of(key).map(|i| self.opt_mut(i))
    }

    /// `DynamicPrintConfig(const StaticPrintConfig &rhs)`: each key of `keys_ref()` cloned.
    #[must_use]
    fn to_dynamic(&self) -> Config {
        let mut out = Config::default();
        for key in Self::keys_ref() {
            if let Some(o) = self.optptr(key) {
                out.insert(key, o.to_value());
            }
        }
        out
    }
}

/// `UnknownOptionException(key)`.
fn unknown(key: &str) -> ConfigError {
    ConfigError(format!("Unknown option exception: {key}"))
}

/// `ConfigBase`, over the three calls a config answers itself.
pub trait ConfigBase {
    /// `option(key)`.
    fn option(&self, key: &str) -> Option<&dyn ConfigOption>;
    /// `option(key, create)`: a static class makes nothing; a dynamic config makes a defined key
    /// with its default.
    fn option_mut(&mut self, key: &str, create: bool) -> Option<&mut dyn ConfigOption>;
    /// `keys()`.
    fn keys(&self) -> Vec<String>;

    /// `apply_only(other, keys, ignore_nonexistent)`: each key's option set from `other`'s, made
    /// first where this config can make it. A key this config cannot hold is skipped with
    /// `ignore_nonexistent`, else refused; `key#n` sets value `n` of the vector `key` from
    /// `other`'s `key`.
    ///
    /// # Errors
    ///
    /// "Unknown option exception: <key>", a `set` of another type, and the cases where the C++
    /// dereferences a null option. The keys before the failing one stay applied.
    fn apply_only(
        &mut self,
        other: &dyn ConfigBase,
        keys: &[String],
        ignore_nonexistent: bool,
    ) -> Result<(), ConfigError>
    where
        Self: Sized,
    {
        for key in keys {
            let key = key.as_str();
            if self.option_mut(key, true).is_none() {
                if let Some(n) = key.find('#') {
                    let key2 = &key[..n];
                    let other_opt = other.option(key2).map(ConfigOption::to_value);
                    let mut mine = self.option(key2).is_some_and(|o| o.opt_type().is_vector());
                    if !mine && let Some(other_opt) = &other_opt {
                        let Some(my_opt2) = self
                            .option_mut(key2, true)
                            .filter(|o| o.opt_type().is_vector())
                        else {
                            return Err(ConfigError(format!(
                                "apply_only(): {key2} is not a vector this config can make (the \
                                 C++ dereferences a null option)"
                            )));
                        };
                        if vector_is_empty(&my_opt2.to_value()) {
                            let mut v = my_opt2.to_value();
                            crate::config::resize(&mut v, 1, Some(other_opt))?;
                            my_opt2.set_value(&v)?;
                        }
                        mine = true;
                    }
                    if mine {
                        let index = orca_fmt::parse::atoi(&key.as_bytes()[n + 1..]);
                        if let Some(other_opt) = &other_opt {
                            let Ok(index) = usize::try_from(index) else {
                                return Err(ConfigError(format!(
                                    "apply_only(): {key} has a negative index"
                                )));
                            };
                            if let Some(my_opt2) = self.option_mut(key2, false) {
                                my_opt2.set_at(other_opt, index, index)?;
                            }
                        }
                        continue;
                    }
                }
                if ignore_nonexistent {
                    continue;
                }
                return Err(unknown(key));
            }
            if let Some(other_opt) = other.option(key).map(ConfigOption::to_value)
                && let Some(my_opt) = self.option_mut(key, false)
            {
                my_opt.set_value(&other_opt)?;
            }
        }
        Ok(())
    }

    /// `apply(other, ignore_nonexistent)`: `apply_only` over `other`'s keys.
    ///
    /// # Errors
    ///
    /// As [`ConfigBase::apply_only`].
    fn apply_config(
        &mut self,
        other: &dyn ConfigBase,
        ignore_nonexistent: bool,
    ) -> Result<(), ConfigError>
    where
        Self: Sized,
    {
        let keys = other.keys();
        self.apply_only(other, &keys, ignore_nonexistent)
    }

    /// `equals(other)`: whether every option both hold is equal.
    ///
    /// # Errors
    ///
    /// The virtual `operator==`'s, for options of two types under one key.
    fn equals(&self, other: &dyn ConfigBase) -> Result<bool, ConfigError>
    where
        Self: Sized,
    {
        for key in self.keys() {
            if let (Some(a), Some(b)) = (self.option(&key), other.option(&key))
                && !a.eq_opt(b)?
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// `diff(other)`: the keys both hold whose options differ, in this config's order.
    ///
    /// # Errors
    ///
    /// As [`ConfigBase::equals`].
    fn diff(&self, other: &dyn ConfigBase) -> Result<Vec<String>, ConfigError>
    where
        Self: Sized,
    {
        let mut out = Vec::new();
        for key in self.keys() {
            if let (Some(a), Some(b)) = (self.option(&key), other.option(&key))
                && !a.eq_opt(b)?
            {
                out.push(key);
            }
        }
        Ok(out)
    }

    /// `equal(other)`: the keys both hold whose options are equal.
    ///
    /// # Errors
    ///
    /// As [`ConfigBase::equals`].
    fn equal(&self, other: &dyn ConfigBase) -> Result<Vec<String>, ConfigError>
    where
        Self: Sized,
    {
        let mut out = Vec::new();
        for key in self.keys() {
            if let (Some(a), Some(b)) = (self.option(&key), other.option(&key))
                && a.eq_opt(b)?
            {
                out.push(key);
            }
        }
        Ok(out)
    }

    /// `opt_serialize(key)`.
    ///
    /// # Errors
    ///
    /// `serialize()`'s, and a key this config lacks (which the C++ asserts against).
    fn opt_serialize(&self, key: &str) -> Result<String, ConfigError> {
        match self.option(key) {
            Some(o) => o.serialize(),
            None => Err(ConfigError(format!(
                "opt_serialize(): no option {key} (the C++ asserts)"
            ))),
        }
    }

    /// `get_abs_value(key)`: a float, int or bool option's value; a float-or-percent's value when
    /// absolute (a zero `*_line_width` reads `line_width` instead); a percent over its
    /// `ratio_over` option's absolute value, or over 1 when it has none.
    ///
    /// # Errors
    ///
    /// "You can't define an option that need <key> without defining it!" for a missing option,
    /// and the cases where the C++ leaves defined behaviour (a percent over a chain that ends at
    /// another type, an option of no numeric type, a key with no definition).
    fn get_abs_value(&self, key: &str) -> Result<f64, ConfigError> {
        let Some(raw) = self.option(key) else {
            return Err(ConfigError(format!(
                "You can't define an option that need {key} without defining it!"
            )));
        };
        let percent_value = match raw.to_value() {
            OptionValue::Float(v) => return Ok(v),
            OptionValue::Int(v) => return Ok(f64::from(v)),
            OptionValue::Bool(v) => return Ok(if v { 1.0 } else { 0.0 }),
            OptionValue::FloatOrPercent { value, percent } => {
                if value == 0.0 && key.ends_with("_line_width") {
                    return self.get_abs_value("line_width");
                }
                if !percent {
                    return Ok(value);
                }
                Some((value, true))
            }
            OptionValue::Percent(v) => Some((v, false)),
            _ => None,
        };
        let Some(def) = crate::get(key) else {
            return Err(ConfigError(format!(
                "get_abs_value(): {key} has no definition (the C++ asserts)"
            )));
        };
        let Some((value, is_fop)) = percent_value else {
            return Err(ConfigError(format!(
                "get_abs_value(): {key} is no numeric option (the C++ reads a null pointer)"
            )));
        };
        if def.ratio_over.is_empty() {
            // ConfigOptionPercent::get_abs_value(1).
            return Ok(value / 100.0);
        }
        if !is_fop {
            return Err(ConfigError(format!(
                "get_abs_value(): {key} is a percent cast to a float-or-percent (the C++ reads past \
                 it)"
            )));
        }
        let over = self.get_abs_value(def.ratio_over)?;
        Ok(over * value / 100.0)
    }

    /// `get_abs_value(key, ratio_over)` of a float-or-percent option.
    ///
    /// # Errors
    ///
    /// "ConfigBase::get_abs_value(): opt_key is not of coFloatOrPercent", and a missing option
    /// (which the C++ asserts against).
    fn get_abs_value_over(&self, key: &str, ratio_over: f64) -> Result<f64, ConfigError> {
        let Some(raw) = self.option(key) else {
            return Err(ConfigError(format!(
                "get_abs_value(): no option {key} (the C++ asserts)"
            )));
        };
        match raw.to_value() {
            OptionValue::FloatOrPercent { value, percent } => Ok(if percent {
                ratio_over * value / 100.0
            } else {
                value
            }),
            _ => Err(ConfigError(
                "ConfigBase::get_abs_value(): opt_key is not of coFloatOrPercent".into(),
            )),
        }
    }

    /// `set_deserialize_strict(key, value)`: the legacy rename or drop, then the value read into
    /// the option (an alias or a shortcut resolved), with no substitution.
    ///
    /// # Errors
    ///
    /// "Unknown option exception: <key>", "Invalid value provided for parameter <key>: <value>"
    /// for a value the option does not read, `deserialize`'s own, and a key this static class
    /// lacks (where the C++ dereferences a null option).
    fn set_deserialize_strict(&mut self, key: &str, value: &str) -> Result<(), ConfigError> {
        let (mut k, mut v) = (key.to_string(), value.to_string());
        crate::handle_legacy(&mut k, &mut v);
        if k.is_empty() {
            return Ok(());
        }
        if !self.set_deserialize_raw(&k, &v)? {
            return Err(ConfigError(format!(
                "Invalid value provided for parameter {key}: {value}"
            )));
        }
        Ok(())
    }

    /// `set_deserialize_raw(key, value, {Disable})`.
    ///
    /// # Errors
    ///
    /// As [`ConfigBase::set_deserialize_strict`].
    fn set_deserialize_raw(&mut self, key: &str, value: &str) -> Result<bool, ConfigError> {
        let Some(def) = find_def(key) else {
            return Err(unknown(key));
        };
        if !def.shortcut.is_empty() {
            for shortcut in def.shortcut {
                if !self.set_deserialize_raw(shortcut, value)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        let Some(opt) = self.option_mut(def.key, true) else {
            return Err(ConfigError(format!(
                "set_deserialize(): this config has no option {} (the C++ dereferences a null \
                 option)",
                def.key
            )));
        };
        opt.deserialize(value)
    }

    /// `set(key, int, create)`.
    ///
    /// # Errors
    ///
    /// "Unknown option exception: <key>" and "Configbase::set() - conversion from int not
    /// possible".
    fn set_int(&mut self, key: &str, value: i32, create: bool) -> Result<(), ConfigError> {
        let Some(opt) = self.option_mut(key, create) else {
            return Err(unknown(key));
        };
        let v = match opt.opt_type() {
            OptionType::Int => OptionValue::Int(value),
            OptionType::Float => OptionValue::Float(f64::from(value)),
            OptionType::FloatOrPercent => OptionValue::FloatOrPercent {
                value: f64::from(value),
                percent: false,
            },
            OptionType::String => OptionValue::String(value.to_string()),
            _ => {
                return Err(ConfigError(
                    "Configbase::set() - conversion from int not possible".into(),
                ));
            }
        };
        opt.set_value(&v)
    }

    /// `set(key, double, create)`; a text option takes `float_to_string_decimal_point(value)`.
    ///
    /// # Errors
    ///
    /// "Unknown option exception: <key>", "Configbase::set() - conversion from float not
    /// possible", and `float_to_string_decimal_point`'s "conversion failed".
    fn set_float(&mut self, key: &str, value: f64, create: bool) -> Result<(), ConfigError> {
        let Some(opt) = self.option_mut(key, create) else {
            return Err(unknown(key));
        };
        let v = match opt.opt_type() {
            OptionType::Float => OptionValue::Float(value),
            OptionType::FloatOrPercent => OptionValue::FloatOrPercent {
                value,
                percent: false,
            },
            OptionType::String => OptionValue::String(float_to_string_decimal_point(value)?),
            _ => {
                return Err(ConfigError(
                    "Configbase::set() - conversion from float not possible".into(),
                ));
            }
        };
        opt.set_value(&v)
    }
}

/// `float_to_string_decimal_point(value)` as the Windows build has it: `std::to_chars` in the
/// general format with precision 6, into 20 characters.
///
/// # Errors
///
/// "float_to_string_decimal_point conversion failed." when 20 characters do not hold it.
pub fn float_to_string_decimal_point(value: f64) -> Result<String, ConfigError> {
    let mut buf = [0u8; 20];
    match orca_fmt::to_chars::to_chars_precision(
        &mut buf,
        value,
        orca_fmt::to_chars::CharsFormat::General,
        6,
    ) {
        Ok(n) => Ok(String::from_utf8_lossy(&buf[..n]).into_owned()),
        Err(_) => Err(ConfigError(
            "float_to_string_decimal_point conversion failed.".into(),
        )),
    }
}

/// Whether a vector value holds no value.
fn vector_is_empty(v: &OptionValue) -> bool {
    crate::config::vec_len(v) == Some(0)
}

impl<T: StaticPrintConfig> ConfigBase for T {
    fn option(&self, key: &str) -> Option<&dyn ConfigOption> {
        self.optptr(key)
    }
    fn option_mut(&mut self, key: &str, _create: bool) -> Option<&mut dyn ConfigOption> {
        self.optptr_mut(key)
    }
    fn keys(&self) -> Vec<String> {
        Self::keys_ref().iter().map(|k| (*k).to_string()).collect()
    }
}

impl ConfigBase for Config {
    fn option(&self, key: &str) -> Option<&dyn ConfigOption> {
        self.get(key).map(|v| v as &dyn ConfigOption)
    }
    fn option_mut(&mut self, key: &str, create: bool) -> Option<&mut dyn ConfigOption> {
        let v = if create {
            self.create(key)
        } else {
            self.get_mut(key)
        };
        v.map(|v| v as &mut dyn ConfigOption)
    }
    fn keys(&self) -> Vec<String> {
        Config::keys(self)
    }
}

/// `DynamicPrintConfig::full_print_config()`: every key of `FullPrintConfig` at its default.
#[must_use]
pub fn full_print_config() -> Config {
    FullPrintConfig::defaults().to_dynamic()
}

/// `DynamicPrintConfig::new_from_defaults_keys(keys)`: a config holding each key, at
/// `FullPrintConfig`'s default where it has the key, else at the definition's.
///
/// # Errors
///
/// "Unknown option exception: <key>" for a key no option has.
pub fn new_from_defaults_keys(keys: &[String]) -> Result<Config, ConfigError> {
    let mut out = Config::default();
    out.apply_only(FullPrintConfig::defaults(), keys, false)?;
    Ok(out)
}
