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
// Carries OrcaSlicer code: the ConfigOption classes of src/libslic3r/Config.hpp (ConfigOption,
// ConfigOptionSingle, ConfigOptionVector, ConfigOptionFloat through ConfigOptionEnumsGenericTempl:
// their type, nullable, is_nil, get_at, getInt, getFloat, getBool, set, operator== (both the typed
// one and the virtual one), hash, get_abs_value and ConfigOptionVector::apply_override), the
// std::hash specializations of FloatOrPercent, Vec2d and Vec3d (Config.hpp) and of Vec2ds and
// std::vector<int> (src/libslic3r/Point.hpp), and is_approx and EPSILON (src/libslic3r/libslic3r.h)
// of OrcaSlicer v2.4.2, commit 8500fcd, translated on 2026-11-13 (orca-o3-front-1); their copyright
// is the OrcaSlicer contributors', under the AGPL-3.0.
//
// Carries Boost code: hash_combine and hash_mix are translated from boost/container_hash/hash.hpp
// and boost/container_hash/detail/hash_mix.hpp of Boost 1.84 (the 64-bit mix), on 2026-11-13; their
// copyright is Daniel James's (2005-2014) and Peter Dimov's (2021, 2022), under the BSL-1.0
// (licenses/BSL-1.0.txt).

//! The options of a static print config, one type per `ConfigOption` class of Config.hpp
//! ([`ConfigOptionFloat`] and the rest), and [`ConfigOption`], what the C++ calls through a
//! `ConfigOption*`.
//!
//! - Each type holds what its class holds (`value`, or `values`, a nullable vector type with
//!   `NULLABLE` true), and its `PartialEq` is the class's own `operator==` (the one a static
//!   class's `operator==` calls field by field): a `ConfigOptionFloat` and a
//!   `ConfigOptionFloatOrPercent` compare with `is_approx` (1e-4), a `ConfigOptionPercent`
//!   exactly, and a nullable vector of doubles takes two nils as equal and the rest with
//!   `is_approx`.
//! - [`ConfigOption`] is the virtual interface: `type()`, `nullable()`, `clone()` (as the
//!   dynamic [`OptionValue`] the moved [`Config`](crate::config::Config) holds), `set()`, the
//!   virtual `operator==` (where `ConfigOptionPercent` compares with `is_approx`, as it inherits
//!   `ConfigOptionFloat`'s), `hash()`, `serialize()`, `deserialize()` and `getInt()`,
//!   `getFloat()` and `getBool()`, each throwing the C++'s text as a [`ConfigError`].
//!   [`OptionValue`] has it too, as the option classes a `DynamicConfig` makes.
//! - [`hash_combine`] is Boost 1.84's: `hash_mix(seed + 0x9e3779b9 + h)` with the 64-bit mix.
//!   Each option's `hash()` is MSVC's `std::hash` (FNV-1a, orca-stl's [`StdHashable`]) of its
//!   value, a vector's folded with [`hash_combine`] from 0, a `FloatOrPercent` xored with
//!   0x9e3779b9 when it is a percent, an enum over the bytes of its type (one for the `unsigned
//!   char` enums), a `Vec2ds` group with Point.hpp's own fold.
//! - A static `ConfigOptionEnumsGeneric` has no names (its `keys_map` stays null, since `set()`
//!   copies only the values), so it serializes each value as "" and cannot deserialize: the C++
//!   dereferences the null map there, and this gives a [`ConfigError`] instead.
//!
//! Left out: `operator<`, `vserialize`, `set(vector)`, `append`, `set_with_*`, `set_only_diff`,
//! `overriden_by` and the `FloatsOrPercents`, `Point3` and `IntsGroups` types, which no static
//! class holds; a later piece that needs one translates it in its own crate.

use std::any::Any;
use std::fmt::Debug;

use orca_stl::StdHashable;

use crate::config::{set_at, set_value};
use crate::value::EnumKeys;
use crate::{ConfigError, OptionType, OptionValue};

/// `Vec2d`, as a point option holds it.
pub type Vec2d = [f64; 2];

/// `EPSILON` (libslic3r.h).
pub const EPSILON: f64 = 1e-4;

/// `is_approx(value, test_value)`: `fabs(value - test_value) < EPSILON`.
#[must_use]
pub fn is_approx(value: f64, test_value: f64) -> bool {
    orca_crt::fabs(value - test_value) < EPSILON
}

/// `boost::hash_detail::hash_mix` for a 64-bit `size_t`.
#[must_use]
pub fn hash_mix(mut x: u64) -> u64 {
    const M: u64 = 0x0e98_46af_9b1a_615d;
    x ^= x >> 32;
    x = x.wrapping_mul(M);
    x ^= x >> 32;
    x = x.wrapping_mul(M);
    x ^= x >> 28;
    x
}

/// `boost::hash_combine(seed, h)` of a `size_t` `h` (whose `boost::hash` is itself).
pub fn hash_combine(seed: &mut u64, h: u64) {
    *seed = hash_mix(seed.wrapping_add(0x9e37_79b9).wrapping_add(h));
}

/// `std::hash<double>`.
fn hash_f64(v: f64) -> u64 {
    v.std_hash()
}

/// `std::hash<Slic3r::Vec2d>` (Config.hpp).
#[must_use]
pub fn hash_vec2d(v: &Vec2d) -> u64 {
    let mut seed = hash_f64(v[0]);
    hash_combine(&mut seed, hash_f64(v[1]));
    seed
}

/// `std::hash<Slic3r::Vec3d>` (Config.hpp).
fn hash_vec3d(v: &[f64; 3]) -> u64 {
    let mut seed = hash_f64(v[0]);
    hash_combine(&mut seed, hash_f64(v[1]));
    hash_combine(&mut seed, hash_f64(v[2]));
    seed
}

/// `std::hash<Slic3r::FloatOrPercent>` (Config.hpp).
fn hash_float_or_percent(value: f64, percent: bool) -> u64 {
    let seed = hash_f64(value);
    if percent { seed ^ 0x9e37_79b9 } else { seed }
}

/// Point.hpp's fold: `seed ^= h + 0x9e3779b9 + (seed << 6) + (seed >> 2)`.
fn fold(seed: u64, h: u64) -> u64 {
    seed ^ h
        .wrapping_add(0x9e37_79b9)
        .wrapping_add(seed << 6)
        .wrapping_add(seed >> 2)
}

/// `std::hash<Slic3r::Vec2ds>` (Point.hpp).
#[must_use]
pub fn hash_vec2ds(group: &[Vec2d]) -> u64 {
    let mut seed = 0u64;
    for p in group {
        seed = fold(seed, hash_f64(p[0]));
        seed = fold(seed, hash_f64(p[1]));
    }
    seed
}

/// `std::hash<std::vector<int>>` (Point.hpp): each value hashed as a double.
fn hash_ints_group(group: &[i32]) -> u64 {
    group
        .iter()
        .fold(0u64, |seed, v| fold(seed, hash_f64(f64::from(*v))))
}

/// `ConfigOptionVector<T>::hash`: each value's hash combined from 0.
fn hash_each<T>(values: &[T], h: impl Fn(&T) -> u64) -> u64 {
    let mut seed = 0u64;
    for v in values {
        hash_combine(&mut seed, h(v));
    }
    seed
}

/// `ConfigOptionFloatsTempl::vectors_equal`: exact for a vector that takes no nil; two nils
/// equal and the rest with `is_approx` for one that does.
fn floats_equal(a: &[f64], b: &[f64], nullable: bool) -> bool {
    if nullable {
        a.len() == b.len()
            && a.iter()
                .zip(b)
                .all(|(x, y)| (x.is_nan() && y.is_nan()) || is_approx(*x, *y))
    } else {
        a == b
    }
}

/// `FloatOrPercent`'s `operator==`.
fn fop_equal(a: (f64, bool), b: (f64, bool)) -> bool {
    is_approx(a.0, b.0) && a.1 == b.1
}

fn incompatible(class: &str) -> ConfigError {
    ConfigError(format!("{class}: Comparing incompatible types"))
}

fn bad_type(what: &str) -> ConfigError {
    ConfigError(format!("Calling ConfigOption::{what} ConfigOption"))
}

/// What the C++ calls through a `ConfigOption*`.
pub trait ConfigOption: Debug + Any {
    /// `type()`.
    fn opt_type(&self) -> OptionType;

    /// `nullable()`.
    fn nullable(&self) -> bool {
        false
    }

    /// `is_nil()`: a scalar is never nil; a vector when every value is.
    fn is_nil(&self) -> bool {
        false
    }

    /// `clone()`, as the value a `DynamicConfig` holds.
    fn to_value(&self) -> OptionValue;

    /// `set(rhs)`: the value of an option of the same type.
    ///
    /// # Errors
    ///
    /// The class's "Assigning an incompatible type" for another type.
    fn set_value(&mut self, rhs: &OptionValue) -> Result<(), ConfigError>;

    /// `set(rhs)` of any option.
    ///
    /// # Errors
    ///
    /// As [`ConfigOption::set_value`].
    fn set(&mut self, rhs: &dyn ConfigOption) -> Result<(), ConfigError> {
        self.set_value(&rhs.to_value())
    }

    /// The virtual `operator==(const ConfigOption &rhs)` of this option's class.
    ///
    /// # Errors
    ///
    /// The class's "Comparing incompatible types" for another type.
    fn eq_value(&self, rhs: &OptionValue) -> Result<bool, ConfigError>;

    /// The virtual `operator==` against any option.
    ///
    /// # Errors
    ///
    /// As [`ConfigOption::eq_value`].
    fn eq_opt(&self, rhs: &dyn ConfigOption) -> Result<bool, ConfigError> {
        self.eq_value(&rhs.to_value())
    }

    /// `hash()`.
    fn hash(&self) -> u64;

    /// `serialize()`.
    ///
    /// # Errors
    ///
    /// The C++'s `ConfigurationError`: a nil in a vector that takes none.
    fn serialize(&self) -> Result<String, ConfigError> {
        self.to_value().serialize()
    }

    /// `deserialize(str, false)`: whether the text was read.
    ///
    /// # Errors
    ///
    /// The C++'s `ConfigurationError` ("nil" in a vector that takes none).
    fn deserialize(&mut self, s: &str) -> Result<bool, ConfigError>;

    /// `getInt()`.
    ///
    /// # Errors
    ///
    /// `BadOptionTypeException` for a type that has no int.
    fn get_int(&self) -> Result<i32, ConfigError> {
        Err(bad_type("getInt on a non-int"))
    }

    /// `getFloat()`.
    ///
    /// # Errors
    ///
    /// `BadOptionTypeException` for a type that has no float.
    fn get_float(&self) -> Result<f64, ConfigError> {
        Err(bad_type("getFloat on a non-float"))
    }

    /// `getBool()`.
    ///
    /// # Errors
    ///
    /// `BadOptionTypeException` for a type that has no bool.
    fn get_bool(&self) -> Result<bool, ConfigError> {
        Err(bad_type("getBool on a non-boolean"))
    }

    /// `set_at(rhs, i, j)` of a vector option (`ConfigOptionVectorBase`).
    ///
    /// # Errors
    ///
    /// `ConfigOptionVector::set_at()`'s; "not a vector" for a scalar, which the C++ reaches only
    /// through a failed `dynamic_cast`.
    fn set_at(&mut self, rhs: &OptionValue, i: usize, j: usize) -> Result<(), ConfigError> {
        if !self.opt_type().is_vector() {
            return Err(ConfigError(
                "set_at() of an option that is not a vector".into(),
            ));
        }
        let mut v = self.to_value();
        set_at(&mut v, rhs, i, j)?;
        self.set_value(&v)
    }

    /// The option as `Any`, for a typed look.
    fn as_any(&self) -> &dyn Any;
}

// ---------------------------------------------------------------- scalars

/// `ConfigOptionFloat`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConfigOptionFloat {
    pub value: f64,
}

impl ConfigOptionFloat {
    /// `ConfigOptionFloat(value)`.
    #[must_use]
    pub fn new(value: f64) -> Self {
        Self { value }
    }
}

impl PartialEq for ConfigOptionFloat {
    /// `is_approx(value, rhs.value)`.
    fn eq(&self, rhs: &Self) -> bool {
        is_approx(self.value, rhs.value)
    }
}

/// `ConfigOptionPercent`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConfigOptionPercent {
    pub value: f64,
}

impl ConfigOptionPercent {
    /// `ConfigOptionPercent(value)`.
    #[must_use]
    pub fn new(value: f64) -> Self {
        Self { value }
    }

    /// `get_abs_value(ratio_over)`: `ratio_over * value / 100`.
    #[must_use]
    pub fn get_abs_value(&self, ratio_over: f64) -> f64 {
        ratio_over * self.value / 100.0
    }
}

impl PartialEq for ConfigOptionPercent {
    /// `value == rhs.value`, exactly (its own `operator==`, unlike its virtual one).
    fn eq(&self, rhs: &Self) -> bool {
        self.value == rhs.value
    }
}

/// `ConfigOptionFloatOrPercent`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConfigOptionFloatOrPercent {
    pub value: f64,
    pub percent: bool,
}

impl ConfigOptionFloatOrPercent {
    /// `ConfigOptionFloatOrPercent(value, percent)`.
    #[must_use]
    pub fn new(value: f64, percent: bool) -> Self {
        Self { value, percent }
    }

    /// `get_abs_value(ratio_over)`: `ratio_over * value / 100` for a percent, else the value.
    #[must_use]
    pub fn get_abs_value(&self, ratio_over: f64) -> f64 {
        if self.percent {
            ratio_over * self.value / 100.0
        } else {
            self.value
        }
    }
}

impl PartialEq for ConfigOptionFloatOrPercent {
    fn eq(&self, rhs: &Self) -> bool {
        fop_equal((self.value, self.percent), (rhs.value, rhs.percent))
    }
}

/// `ConfigOptionInt`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConfigOptionInt {
    pub value: i32,
}

impl ConfigOptionInt {
    /// `ConfigOptionInt(value)`.
    #[must_use]
    pub fn new(value: i32) -> Self {
        Self { value }
    }
}

/// `ConfigOptionString`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigOptionString {
    pub value: String,
}

impl ConfigOptionString {
    /// `ConfigOptionString(value)`.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
        }
    }
}

/// `ConfigOptionPoint`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ConfigOptionPoint {
    pub value: Vec2d,
}

impl ConfigOptionPoint {
    /// `ConfigOptionPoint(value)`.
    #[must_use]
    pub fn new(value: Vec2d) -> Self {
        Self { value }
    }
}

/// `ConfigOptionBool`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConfigOptionBool {
    pub value: bool,
}

impl ConfigOptionBool {
    /// `ConfigOptionBool(value)`.
    #[must_use]
    pub fn new(value: bool) -> Self {
        Self { value }
    }
}

/// An enum of PrintConfig.hpp, as `ConfigOptionEnum<T>` holds it.
pub trait ConfigEnum: Copy + Debug + Default + PartialEq + 'static {
    /// The enum's name.
    const NAME: &'static str;
    /// `sizeof(T)`: 1 for the `unsigned char` enums, else 4. `std::hash<T>` hashes these bytes.
    const BYTES: usize;
    /// An option whose definition holds the enum's names (`ConfigOptionEnum<T>::get_enum_values()`),
    /// or `None` for an enum no static option holds.
    const KEYS_OPTION: Option<&'static str>;
    /// The enumerator of a value, `None` for a value the enum does not declare.
    fn from_int(v: i32) -> Option<Self>;
    /// The enumerator's value.
    fn to_int(self) -> i32;

    /// `ConfigOptionEnum<T>::get_enum_values()`.
    #[must_use]
    fn keys_map() -> Option<EnumKeys> {
        Self::KEYS_OPTION
            .and_then(crate::get)
            .and_then(|d| d.enum_keys_map)
    }
}

impl ConfigEnum for crate::PrinterTechnology {
    const NAME: &'static str = "PrinterTechnology";
    const BYTES: usize = 1;
    const KEYS_OPTION: Option<&'static str> = Some("printer_technology");
    fn from_int(v: i32) -> Option<Self> {
        Some(match v {
            0 => Self::Fff,
            1 => Self::Sla,
            2 => Self::Unknown,
            3 => Self::Any,
            _ => return None,
        })
    }
    fn to_int(self) -> i32 {
        match self {
            Self::Fff => 0,
            Self::Sla => 1,
            Self::Unknown => 2,
            Self::Any => 3,
        }
    }
}

impl Default for crate::PrinterTechnology {
    /// `static_cast<PrinterTechnology>(0)`: `ptFFF`.
    fn default() -> Self {
        Self::Fff
    }
}

/// `std::hash<T>` of an enum: FNV-1a over its `T::BYTES` bytes.
fn hash_enum<T: ConfigEnum>(v: T) -> u64 {
    let bytes = v.to_int().to_le_bytes();
    orca_stl::unordered::hash_bytes(&bytes[..T::BYTES])
}

/// `ConfigOptionEnum<T>`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ConfigOptionEnum<T: ConfigEnum> {
    pub value: T,
}

impl<T: ConfigEnum> ConfigOptionEnum<T> {
    /// `ConfigOptionEnum<T>(value)`.
    #[must_use]
    pub fn new(value: T) -> Self {
        Self { value }
    }

    fn of_int(v: i32) -> Result<T, ConfigError> {
        T::from_int(v).ok_or_else(|| {
            ConfigError(format!(
                "ConfigOptionEnum<{}>: {v} is no value of the enum (the C++ would cast it)",
                T::NAME
            ))
        })
    }
}

// ---------------------------------------------------------------- vectors

/// The methods every `ConfigOptionVector<T>` has.
macro_rules! vector_common {
    ($ty:ident $(<$n:ident>)?, $elem:ty) => {
        impl$(<const $n: bool>)? $ty$(<$n>)? {
            /// `ConfigOptionVector(values)`.
            #[must_use]
            pub fn new(values: Vec<$elem>) -> Self {
                Self { values }
            }

            /// `get_at(i)`: value `i`, or the first past the end.
            ///
            /// # Panics
            ///
            /// For an empty vector, which the C++ asserts against.
            #[must_use]
            pub fn get_at(&self, i: usize) -> &$elem {
                self.values.get(i).unwrap_or_else(|| &self.values[0])
            }

            /// `size()`.
            #[must_use]
            pub fn size(&self) -> usize {
                self.values.len()
            }

            /// `empty()`.
            #[must_use]
            pub fn empty(&self) -> bool {
                self.values.is_empty()
            }
        }
    };
}

/// `ConfigOptionFloatsTempl<NULLABLE>`.
#[derive(Clone, Debug, Default)]
pub struct ConfigOptionFloatsTempl<const NULLABLE: bool> {
    pub values: Vec<f64>,
}
/// `ConfigOptionFloats`.
pub type ConfigOptionFloats = ConfigOptionFloatsTempl<false>;
/// `ConfigOptionFloatsNullable`.
pub type ConfigOptionFloatsNullable = ConfigOptionFloatsTempl<true>;
vector_common!(ConfigOptionFloatsTempl<N>, f64);

impl<const N: bool> ConfigOptionFloatsTempl<N> {
    /// `nil_value()`: NaN.
    #[must_use]
    pub fn nil_value() -> f64 {
        f64::NAN
    }

    /// `is_nil(idx)`.
    #[must_use]
    pub fn is_nil_at(&self, idx: usize) -> bool {
        self.values[idx].is_nan()
    }
}

impl<const N: bool> PartialEq for ConfigOptionFloatsTempl<N> {
    fn eq(&self, rhs: &Self) -> bool {
        floats_equal(&self.values, &rhs.values, N)
    }
}

/// `ConfigOptionPercentsTempl<NULLABLE>`.
#[derive(Clone, Debug, Default)]
pub struct ConfigOptionPercentsTempl<const NULLABLE: bool> {
    pub values: Vec<f64>,
}
/// `ConfigOptionPercents`.
pub type ConfigOptionPercents = ConfigOptionPercentsTempl<false>;
/// `ConfigOptionPercentsNullable`.
pub type ConfigOptionPercentsNullable = ConfigOptionPercentsTempl<true>;
vector_common!(ConfigOptionPercentsTempl<N>, f64);

impl<const N: bool> ConfigOptionPercentsTempl<N> {
    /// `is_nil(idx)`.
    #[must_use]
    pub fn is_nil_at(&self, idx: usize) -> bool {
        self.values[idx].is_nan()
    }
}

impl<const N: bool> PartialEq for ConfigOptionPercentsTempl<N> {
    fn eq(&self, rhs: &Self) -> bool {
        floats_equal(&self.values, &rhs.values, N)
    }
}

/// `ConfigOptionIntsTempl<NULLABLE>`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigOptionIntsTempl<const NULLABLE: bool> {
    pub values: Vec<i32>,
}
/// `ConfigOptionInts`.
pub type ConfigOptionInts = ConfigOptionIntsTempl<false>;
/// `ConfigOptionIntsNullable`.
pub type ConfigOptionIntsNullable = ConfigOptionIntsTempl<true>;
vector_common!(ConfigOptionIntsTempl<N>, i32);

impl<const N: bool> ConfigOptionIntsTempl<N> {
    /// `nil_value()`: `INT_MAX`.
    #[must_use]
    pub fn nil_value() -> i32 {
        i32::MAX
    }

    /// `is_nil(idx)`.
    #[must_use]
    pub fn is_nil_at(&self, idx: usize) -> bool {
        self.values[idx] == i32::MAX
    }
}

/// `ConfigOptionStrings`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigOptionStrings {
    pub values: Vec<String>,
}
vector_common!(ConfigOptionStrings, String);

/// `ConfigOptionPoints`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigOptionPoints {
    pub values: Vec<Vec2d>,
}
vector_common!(ConfigOptionPoints, Vec2d);

/// `ConfigOptionBoolsTempl<NULLABLE>`: each value 0, 1, or 255 for nil.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigOptionBoolsTempl<const NULLABLE: bool> {
    pub values: Vec<u8>,
}
/// `ConfigOptionBools`.
pub type ConfigOptionBools = ConfigOptionBoolsTempl<false>;
/// `ConfigOptionBoolsNullable`.
pub type ConfigOptionBoolsNullable = ConfigOptionBoolsTempl<true>;

impl<const N: bool> ConfigOptionBoolsTempl<N> {
    /// `ConfigOptionBoolsTempl(values)`.
    #[must_use]
    pub fn new(values: Vec<u8>) -> Self {
        Self { values }
    }

    /// `get_at(i) const`: value `i` (or the first past the end) `!= 0`.
    ///
    /// # Panics
    ///
    /// For an empty vector, which the C++ asserts against.
    #[must_use]
    pub fn get_at(&self, i: usize) -> bool {
        self.values
            .get(i)
            .copied()
            .unwrap_or_else(|| self.values[0])
            != 0
    }

    /// `size()`.
    #[must_use]
    pub fn size(&self) -> usize {
        self.values.len()
    }

    /// `empty()`.
    #[must_use]
    pub fn empty(&self) -> bool {
        self.values.is_empty()
    }

    /// `nil_value()`: 255.
    #[must_use]
    pub fn nil_value() -> u8 {
        u8::MAX
    }

    /// `is_nil(idx)`.
    #[must_use]
    pub fn is_nil_at(&self, idx: usize) -> bool {
        self.values[idx] == u8::MAX
    }
}

/// `ConfigOptionEnumsGenericTempl<NULLABLE>` as a static config holds it: its `keys_map` null.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigOptionEnumsGenericTempl<const NULLABLE: bool> {
    pub values: Vec<i32>,
}
/// `ConfigOptionEnumsGeneric`.
pub type ConfigOptionEnumsGeneric = ConfigOptionEnumsGenericTempl<false>;
/// `ConfigOptionEnumsGenericNullable`.
pub type ConfigOptionEnumsGenericNullable = ConfigOptionEnumsGenericTempl<true>;
vector_common!(ConfigOptionEnumsGenericTempl<N>, i32);

/// `ConfigOptionPointsGroups`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigOptionPointsGroups {
    pub values: Vec<Vec<Vec2d>>,
}
vector_common!(ConfigOptionPointsGroups, Vec<Vec2d>);

// ---------------------------------------------------------------- ConfigOption for each type

/// `rhs.type() != this->type()` before a `set`.
fn check_set(ty: OptionType, rhs: &OptionValue, class: &str) -> Result<(), ConfigError> {
    if rhs.ty() == ty {
        Ok(())
    } else {
        Err(ConfigError(format!(
            "{class}: Assigning an incompatible type"
        )))
    }
}

/// `deserialize` through the dynamic value: the same reading, then the value taken back.
fn deserialize_via<T: ConfigOption + ?Sized>(opt: &mut T, s: &str) -> Result<bool, ConfigError> {
    let mut v = opt.to_value();
    let read = v.deserialize(s)?;
    opt.set_value(&v)?;
    Ok(read)
}

macro_rules! scalar_option {
    ($ty:ident, $variant:ident, $class:literal, $eq_class:literal, own |$o:ident| $own:expr, eq |$s:ident, $r:ident| $eq:expr, hash |$h:ident| $hash:expr) => {
        impl ConfigOption for $ty {
            fn opt_type(&self) -> OptionType {
                OptionType::$variant
            }
            fn to_value(&self) -> OptionValue {
                let $o = &self.value;
                OptionValue::$variant($own)
            }
            fn set_value(&mut self, rhs: &OptionValue) -> Result<(), ConfigError> {
                check_set(OptionType::$variant, rhs, $class)?;
                if let OptionValue::$variant($o) = rhs {
                    self.value = $own;
                }
                Ok(())
            }
            fn eq_value(&self, rhs: &OptionValue) -> Result<bool, ConfigError> {
                match rhs {
                    OptionValue::$variant($r) => {
                        let $s = &self.value;
                        Ok($eq)
                    }
                    _ => Err(incompatible($eq_class)),
                }
            }
            fn hash(&self) -> u64 {
                let $h = &self.value;
                $hash
            }
            fn deserialize(&mut self, s: &str) -> Result<bool, ConfigError> {
                deserialize_via(self, s)
            }
            fn as_any(&self) -> &dyn Any {
                self
            }
            scalar_option!(@getters $variant);
        }
    };
    (@getters Float) => {
        fn get_float(&self) -> Result<f64, ConfigError> {
            Ok(self.value)
        }
    };
    (@getters Percent) => {
        fn get_float(&self) -> Result<f64, ConfigError> {
            Ok(self.value)
        }
    };
    (@getters Int) => {
        fn get_int(&self) -> Result<i32, ConfigError> {
            Ok(self.value)
        }
    };
    (@getters Bool) => {
        fn get_bool(&self) -> Result<bool, ConfigError> {
            Ok(self.value)
        }
    };
    (@getters $other:ident) => {};
}

scalar_option!(
    ConfigOptionFloat,
    Float,
    "ConfigOptionSingle",
    "ConfigOptionFloat",
    own | v | *v,
    eq | a,
    b | is_approx(*a, *b),
    hash | v | hash_f64(*v)
);
// ConfigOptionPercent inherits ConfigOptionFloat's virtual operator==, which compares as floats.
scalar_option!(
    ConfigOptionPercent,
    Percent,
    "ConfigOptionSingle",
    "ConfigOptionFloat",
    own | v | *v,
    eq | a,
    b | is_approx(*a, *b),
    hash | v | hash_f64(*v)
);
scalar_option!(
    ConfigOptionInt,
    Int,
    "ConfigOptionSingle",
    "ConfigOptionSingle",
    own | v | *v,
    eq | a,
    b | a == b,
    hash | v | v.std_hash()
);
scalar_option!(
    ConfigOptionString,
    String,
    "ConfigOptionSingle",
    "ConfigOptionSingle",
    own | v | v.clone(),
    eq | a,
    b | a == b,
    hash | v | v.as_str().std_hash()
);
scalar_option!(
    ConfigOptionPoint,
    Point,
    "ConfigOptionSingle",
    "ConfigOptionSingle",
    own | v | *v,
    eq | a,
    b | a == b,
    hash | v | hash_vec2d(v)
);
scalar_option!(
    ConfigOptionBool,
    Bool,
    "ConfigOptionSingle",
    "ConfigOptionSingle",
    own | v | *v,
    eq | a,
    b | a == b,
    hash | v | v.std_hash()
);

impl ConfigOption for ConfigOptionFloatOrPercent {
    fn opt_type(&self) -> OptionType {
        OptionType::FloatOrPercent
    }
    fn to_value(&self) -> OptionValue {
        OptionValue::FloatOrPercent {
            value: self.value,
            percent: self.percent,
        }
    }
    fn set_value(&mut self, rhs: &OptionValue) -> Result<(), ConfigError> {
        check_set(
            OptionType::FloatOrPercent,
            rhs,
            "ConfigOptionFloatOrPercent",
        )?;
        if let OptionValue::FloatOrPercent { value, percent } = rhs {
            self.value = *value;
            self.percent = *percent;
        }
        Ok(())
    }
    fn eq_value(&self, rhs: &OptionValue) -> Result<bool, ConfigError> {
        match rhs {
            OptionValue::FloatOrPercent { value, percent } => {
                Ok(fop_equal((self.value, self.percent), (*value, *percent)))
            }
            _ => Err(incompatible("ConfigOptionFloatOrPercent")),
        }
    }
    fn hash(&self) -> u64 {
        hash_float_or_percent(self.value, self.percent)
    }
    fn deserialize(&mut self, s: &str) -> Result<bool, ConfigError> {
        deserialize_via(self, s)
    }
    fn get_float(&self) -> Result<f64, ConfigError> {
        Ok(self.value)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<T: ConfigEnum> ConfigOption for ConfigOptionEnum<T> {
    fn opt_type(&self) -> OptionType {
        OptionType::Enum
    }
    fn to_value(&self) -> OptionValue {
        OptionValue::Enum {
            value: self.value.to_int(),
            keys: T::keys_map(),
        }
    }
    fn set_value(&mut self, rhs: &OptionValue) -> Result<(), ConfigError> {
        check_set(OptionType::Enum, rhs, "ConfigOptionEnum<T>")?;
        if let OptionValue::Enum { value, .. } = rhs {
            self.value = Self::of_int(*value)?;
        }
        Ok(())
    }
    fn eq_value(&self, rhs: &OptionValue) -> Result<bool, ConfigError> {
        match rhs {
            OptionValue::Enum { value, .. } => Ok(self.value.to_int() == *value),
            _ => Err(incompatible("ConfigOptionEnum<T>")),
        }
    }
    fn hash(&self) -> u64 {
        hash_enum(self.value)
    }
    fn deserialize(&mut self, s: &str) -> Result<bool, ConfigError> {
        deserialize_via(self, s)
    }
    fn get_int(&self) -> Result<i32, ConfigError> {
        Ok(self.value.to_int())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

macro_rules! vector_option {
    (
        $ty:ident $(<$n:ident>)?, $variant:ident, $nullable:expr,
        to |$t:ident| $to:expr,
        from |$f:ident| $from:pat => $take:expr,
        eq $eq_class:literal |$a:ident, $b:ident| $eq:expr,
        hash |$h:ident| $hash:expr,
        nil |$z:ident| $nil:expr
    ) => {
        impl$(<const $n: bool>)? ConfigOption for $ty$(<$n>)? {
            fn opt_type(&self) -> OptionType {
                OptionType::$variant
            }
            fn nullable(&self) -> bool {
                $nullable
            }
            fn is_nil(&self) -> bool {
                let $z = &self.values;
                $nil
            }
            fn to_value(&self) -> OptionValue {
                let $t = self;
                $to
            }
            fn set_value(&mut self, rhs: &OptionValue) -> Result<(), ConfigError> {
                check_set(OptionType::$variant, rhs, "ConfigOptionVector")?;
                let $f = rhs;
                if let $from = $f {
                    self.values = $take;
                }
                Ok(())
            }
            fn eq_value(&self, rhs: &OptionValue) -> Result<bool, ConfigError> {
                let $b = rhs;
                if let $from = $b {
                    let $a = &self.values;
                    let $b = &$take;
                    Ok($eq)
                } else {
                    Err(incompatible($eq_class))
                }
            }
            fn hash(&self) -> u64 {
                let $h = &self.values;
                $hash
            }
            fn deserialize(&mut self, s: &str) -> Result<bool, ConfigError> {
                deserialize_via(self, s)
            }
            fn as_any(&self) -> &dyn Any {
                self
            }
        }
    };
}

vector_option!(
    ConfigOptionFloatsTempl<N>, Floats, N,
    to |t| OptionValue::Floats { values: t.values.clone(), nullable: N },
    from |f| OptionValue::Floats { values, .. } => values.clone(),
    eq "ConfigOptionFloatsTempl" |a, b| floats_equal(a, b, N),
    hash |v| hash_each(v, |x| hash_f64(*x)),
    nil |v| v.iter().all(|x| x.is_nan())
);
vector_option!(
    ConfigOptionPercentsTempl<N>, Percents, N,
    to |t| OptionValue::Percents { values: t.values.clone(), nullable: N },
    from |f| OptionValue::Percents { values, .. } => values.clone(),
    eq "ConfigOptionFloatsTempl" |a, b| floats_equal(a, b, N),
    hash |v| hash_each(v, |x| hash_f64(*x)),
    nil |v| v.iter().all(|x| x.is_nan())
);
vector_option!(
    ConfigOptionIntsTempl<N>, Ints, N,
    to |t| OptionValue::Ints { values: t.values.clone(), nullable: N },
    from |f| OptionValue::Ints { values, .. } => values.clone(),
    eq "ConfigOptionVector" |a, b| a == b,
    hash |v| hash_each(v, StdHashable::std_hash),
    nil |v| v.iter().all(|x| *x == i32::MAX)
);
vector_option!(
    ConfigOptionStrings, Strings, false,
    to |t| OptionValue::Strings(t.values.clone()),
    from |f| OptionValue::Strings(values) => values.clone(),
    eq "ConfigOptionVector" |a, b| a == b,
    hash |v| hash_each(v, |x| x.as_str().std_hash()),
    nil |_v| false
);
vector_option!(
    ConfigOptionPoints, Points, false,
    to |t| OptionValue::Points(t.values.clone()),
    from |f| OptionValue::Points(values) => values.clone(),
    eq "ConfigOptionVector" |a, b| a == b,
    hash |v| hash_each(v, hash_vec2d),
    nil |_v| false
);
vector_option!(
    ConfigOptionBoolsTempl<N>, Bools, N,
    to |t| OptionValue::Bools { values: t.values.clone(), nullable: N },
    from |f| OptionValue::Bools { values, .. } => values.clone(),
    eq "ConfigOptionVector" |a, b| a == b,
    hash |v| hash_each(v, StdHashable::std_hash),
    nil |v| v.iter().all(|x| *x == u8::MAX)
);
vector_option!(
    ConfigOptionPointsGroups, PointsGroups, false,
    to |t| OptionValue::PointsGroups(t.values.clone()),
    from |f| OptionValue::PointsGroups(values) => values.clone(),
    eq "ConfigOptionPointsGroupsTempl" |a, b| a == b,
    hash |v| hash_each(v, |g| hash_vec2ds(g)),
    nil |_v| false
);

impl<const N: bool> ConfigOption for ConfigOptionEnumsGenericTempl<N> {
    fn opt_type(&self) -> OptionType {
        OptionType::Enums
    }
    fn nullable(&self) -> bool {
        N
    }
    fn is_nil(&self) -> bool {
        self.values.iter().all(|x| *x == i32::MAX)
    }
    fn to_value(&self) -> OptionValue {
        OptionValue::Enums {
            values: self.values.clone(),
            keys: None,
            nullable: N,
        }
    }
    fn set_value(&mut self, rhs: &OptionValue) -> Result<(), ConfigError> {
        check_set(OptionType::Enums, rhs, "ConfigOptionEnumGeneric")?;
        if let OptionValue::Enums { values, .. } = rhs {
            self.values = values.clone();
        }
        Ok(())
    }
    fn eq_value(&self, rhs: &OptionValue) -> Result<bool, ConfigError> {
        match rhs {
            OptionValue::Enums { values, .. } => Ok(self.values == *values),
            _ => Err(incompatible("ConfigOptionVector")),
        }
    }
    fn hash(&self) -> u64 {
        hash_each(&self.values, StdHashable::std_hash)
    }
    /// `deserialize(str)`: "" and "nil" items read as the C++ reads them; a name makes the C++
    /// look it up in the null `keys_map`, which this refuses.
    fn deserialize(&mut self, s: &str) -> Result<bool, ConfigError> {
        let mut v = self.to_value();
        if !v.deserialize(s)? {
            return Err(ConfigError(
                "ConfigOptionEnumsGeneric::deserialize of a static option, whose keys_map is null \
                 (the C++ dereferences it)"
                    .into(),
            ));
        }
        self.set_value(&v)?;
        Ok(true)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ---------------------------------------------------------------- the dynamic options

impl ConfigOption for OptionValue {
    fn opt_type(&self) -> OptionType {
        self.ty()
    }
    fn nullable(&self) -> bool {
        match self {
            OptionValue::Floats { nullable, .. }
            | OptionValue::Ints { nullable, .. }
            | OptionValue::Percents { nullable, .. }
            | OptionValue::FloatsOrPercents { nullable, .. }
            | OptionValue::Bools { nullable, .. }
            | OptionValue::Enums { nullable, .. } => *nullable,
            _ => false,
        }
    }
    fn is_nil(&self) -> bool {
        match self {
            OptionValue::Floats { values, .. } | OptionValue::Percents { values, .. } => {
                values.iter().all(|x| x.is_nan())
            }
            OptionValue::FloatsOrPercents { values, .. } => values.iter().all(|x| x.0.is_nan()),
            OptionValue::Ints { values, .. } | OptionValue::Enums { values, .. } => {
                values.iter().all(|x| *x == i32::MAX)
            }
            OptionValue::Bools { values, .. } => values.iter().all(|x| *x == u8::MAX),
            _ => false,
        }
    }
    fn to_value(&self) -> OptionValue {
        self.clone()
    }
    fn set_value(&mut self, rhs: &OptionValue) -> Result<(), ConfigError> {
        set_value(self, rhs)
    }
    fn eq_value(&self, rhs: &OptionValue) -> Result<bool, ConfigError> {
        value_eq(self, rhs)
    }
    fn hash(&self) -> u64 {
        value_hash(self)
    }
    fn serialize(&self) -> Result<String, ConfigError> {
        OptionValue::serialize(self)
    }
    fn deserialize(&mut self, s: &str) -> Result<bool, ConfigError> {
        OptionValue::deserialize(self, s)
    }
    fn get_int(&self) -> Result<i32, ConfigError> {
        match self {
            OptionValue::Int(v) | OptionValue::Enum { value: v, .. } => Ok(*v),
            _ => Err(bad_type("getInt on a non-int")),
        }
    }
    fn get_float(&self) -> Result<f64, ConfigError> {
        match self {
            OptionValue::Float(v)
            | OptionValue::Percent(v)
            | OptionValue::FloatOrPercent { value: v, .. } => Ok(*v),
            _ => Err(bad_type("getFloat on a non-float")),
        }
    }
    fn get_bool(&self) -> Result<bool, ConfigError> {
        match self {
            OptionValue::Bool(v) => Ok(*v),
            _ => Err(bad_type("getBool on a non-boolean")),
        }
    }
    fn set_at(&mut self, rhs: &OptionValue, i: usize, j: usize) -> Result<(), ConfigError> {
        set_at(self, rhs, i, j)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The virtual `operator==` of the option class a value stands for (a dynamic `coEnum` is a
/// `ConfigOptionEnumGeneric` or a `ConfigOptionEnum<T>`: both compare the values).
///
/// # Errors
///
/// The class's "Comparing incompatible types" for values of two types.
pub fn value_eq(a: &OptionValue, b: &OptionValue) -> Result<bool, ConfigError> {
    use OptionValue as V;
    Ok(match (a, b) {
        (V::Float(x), V::Float(y)) | (V::Percent(x), V::Percent(y)) => is_approx(*x, *y),
        (
            V::FloatOrPercent {
                value: x,
                percent: p,
            },
            V::FloatOrPercent {
                value: y,
                percent: q,
            },
        ) => fop_equal((*x, *p), (*y, *q)),
        (
            V::Floats {
                values: x,
                nullable,
            },
            V::Floats { values: y, .. },
        )
        | (
            V::Percents {
                values: x,
                nullable,
            },
            V::Percents { values: y, .. },
        ) => floats_equal(x, y, *nullable),
        (
            V::FloatsOrPercents {
                values: x,
                nullable,
            },
            V::FloatsOrPercents { values: y, .. },
        ) => {
            if *nullable {
                x.len() == y.len()
                    && x.iter()
                        .zip(y)
                        .all(|(p, q)| (p.0.is_nan() && q.0.is_nan()) || fop_equal(*p, *q))
            } else {
                x.len() == y.len() && x.iter().zip(y).all(|(p, q)| fop_equal(*p, *q))
            }
        }
        (V::Int(x), V::Int(y)) => x == y,
        (V::Ints { values: x, .. }, V::Ints { values: y, .. })
        | (V::Enums { values: x, .. }, V::Enums { values: y, .. }) => x == y,
        (V::String(x), V::String(y)) => x == y,
        (V::Strings(x), V::Strings(y)) => x == y,
        (V::Point(x), V::Point(y)) => x == y,
        (V::Points(x), V::Points(y)) => x == y,
        (V::Point3(x), V::Point3(y)) => x == y,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Bools { values: x, .. }, V::Bools { values: y, .. }) => x == y,
        (V::Enum { value: x, .. }, V::Enum { value: y, .. }) => x == y,
        (V::PointsGroups(x), V::PointsGroups(y)) => x == y,
        (V::IntsGroups(x), V::IntsGroups(y)) => x == y,
        _ => {
            return Err(incompatible(match a {
                V::Float(_) | V::Percent(_) => "ConfigOptionFloat",
                V::FloatOrPercent { .. } => "ConfigOptionFloatOrPercent",
                V::Floats { .. } | V::Percents { .. } => "ConfigOptionFloatsTempl",
                V::FloatsOrPercents { .. } => "ConfigOptionFloatsOrPercentsTempl",
                V::Enum { .. } => "ConfigOptionEnumGeneric",
                V::PointsGroups(_) => "ConfigOptionPointsGroupsTempl",
                V::IntsGroups(_) => "ConfigConfigOptionIntsGroups",
                V::Int(_) | V::String(_) | V::Point(_) | V::Point3(_) | V::Bool(_) => {
                    "ConfigOptionSingle"
                }
                _ => "ConfigOptionVector",
            }));
        }
    })
}

/// `hash()` of the option class a value stands for. A dynamic `coEnum` hashes as the `int` of a
/// `ConfigOptionEnumGeneric` (a `ConfigOptionEnum<T>` copied from a static config hashes over
/// its enum's bytes, which the value does not record).
#[must_use]
pub fn value_hash(v: &OptionValue) -> u64 {
    use OptionValue as V;
    match v {
        V::Float(x) | V::Percent(x) => hash_f64(*x),
        V::FloatOrPercent { value, percent } => hash_float_or_percent(*value, *percent),
        V::Floats { values, .. } | V::Percents { values, .. } => {
            hash_each(values, |x| hash_f64(*x))
        }
        V::FloatsOrPercents { values, .. } => {
            hash_each(values, |(x, p)| hash_float_or_percent(*x, *p))
        }
        V::Int(x) | V::Enum { value: x, .. } => x.std_hash(),
        V::Ints { values, .. } | V::Enums { values, .. } => {
            hash_each(values, StdHashable::std_hash)
        }
        V::String(s) => s.as_str().std_hash(),
        V::Strings(values) => hash_each(values, |x| x.as_str().std_hash()),
        V::Point(p) => hash_vec2d(p),
        V::Points(values) => hash_each(values, hash_vec2d),
        V::Point3(p) => hash_vec3d(p),
        V::Bool(b) => b.std_hash(),
        V::Bools { values, .. } => hash_each(values, StdHashable::std_hash),
        V::PointsGroups(groups) => hash_each(groups, |g| hash_vec2ds(g)),
        V::IntsGroups(groups) => hash_each(groups, |g| hash_ints_group(g)),
    }
}

/// `ConfigOption::apply_override(rhs, default_index)`: a scalar (or a vector over a vector that
/// takes no nil) becomes `rhs` when they differ; a vector over a nullable one takes each value of
/// `rhs` that is not nil, and for a nil value its own value `default_index[i] - 1` (its first when
/// that index is past its values, or `default_index` is shorter), after growing to `rhs`'s size.
/// Whether it changed.
///
/// # Errors
///
/// "Cannot override a nullable ConfigOption." for a nullable `self`, and "applied to different
/// types" for another type.
pub fn apply_override(
    this: &mut OptionValue,
    rhs: &OptionValue,
    default_index: &[i32],
) -> Result<bool, ConfigError> {
    if !this.ty().is_vector() {
        if value_eq(this, rhs)? {
            return Ok(false);
        }
        set_value(this, rhs)?;
        return Ok(true);
    }
    if ConfigOption::nullable(this) {
        return Err(ConfigError(
            "Cannot override a nullable ConfigOption.".into(),
        ));
    }
    if rhs.ty() != this.ty() {
        return Err(ConfigError(
            "ConfigOptionVector.apply_override() applied to different types.".into(),
        ));
    }
    if !ConfigOption::nullable(rhs) {
        // std::vector's operator!=, exact.
        if !vector_values_equal(this, rhs) {
            set_value(this, rhs)?;
            return Ok(true);
        }
        return Ok(false);
    }
    let len = crate::config::vec_len(this).unwrap_or(0);
    let rhs_len = crate::config::vec_len(rhs).unwrap_or(0);
    let cnt = orca_stl::min(len, rhs_len);
    if cnt < 1 {
        return Ok(false);
    }
    let default_value = this.clone();
    crate::config::resize(this, rhs_len, None)?;
    let mut modified = false;
    for i in 0..rhs_len {
        if !value_is_nil_at(rhs, i) {
            set_at(this, rhs, i, i)?;
            modified = true;
        } else {
            // `default_index[i] - 1 < default_value.size()` compares as size_t: a 0 index
            // wraps to the largest size_t and fails.
            let k = default_index
                .get(i)
                .and_then(|d| usize::try_from(i64::from(*d) - 1).ok())
                .filter(|k| *k < len);
            set_at(this, &default_value, i, k.unwrap_or(0))?;
        }
    }
    Ok(modified)
}

/// `this->values == rhs->values` of two vectors of one type (`std::vector`'s `==`, exact).
fn vector_values_equal(a: &OptionValue, b: &OptionValue) -> bool {
    use OptionValue as V;
    match (a, b) {
        (V::Floats { values: x, .. }, V::Floats { values: y, .. })
        | (V::Percents { values: x, .. }, V::Percents { values: y, .. }) => x == y,
        (V::FloatsOrPercents { values: x, .. }, V::FloatsOrPercents { values: y, .. }) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| fop_equal(*p, *q))
        }
        _ => value_eq(a, b).unwrap_or(false),
    }
}

/// `is_nil(idx)` of a vector value.
fn value_is_nil_at(v: &OptionValue, i: usize) -> bool {
    use OptionValue as V;
    match v {
        V::Floats { values, .. } | V::Percents { values, .. } => values[i].is_nan(),
        V::FloatsOrPercents { values, .. } => values[i].0.is_nan(),
        V::Ints { values, .. } | V::Enums { values, .. } => values[i] == i32::MAX,
        V::Bools { values, .. } => values[i] == u8::MAX,
        _ => false,
    }
}
