// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Orca's configuration: the definition of every option, the values options hold, the renames
//! and drops Orca applies to the keys it reads, the dynamic config the presets stage builds, and
//! the static print configs the slicing stages read.
//!
//! - [`options`], [`get`], [`has`]: `print_config_def`, every option of OrcaSlicer 2.4.2 with
//!   every field of its `ConfigOptionDef` ([`OptionDef`]), in its order (by key). They are
//!   generated, never typed in: `bun scripts/orca.ts parity gen-config` writes
//!   `src/def_generated.rs` from the configdef probe (`parity/probes/configdef.json`), and
//!   `parity gen-config --check` fails while the file differs from what the probe gives.
//! - [`PRINT_OPTIONS`], [`FILAMENT_OPTIONS`], [`PRINTER_OPTIONS`]: `Preset::print_options()` and
//!   the others, the keys of each preset section; [`LEGACY_KEYS`]: the keys `handle_legacy` names.
//! - [`value`]: [`OptionValue`], the values of Orca's `ConfigOption` types, with their
//!   `serialize` and `deserialize`, the same text both ways.
//! - [`legacy`]: [`handle_legacy`], `PrintConfigDef::handle_legacy`.
//! - [`config`]: [`Config`](config::Config), the `DynamicPrintConfig` the presets stage builds,
//!   and what cli/presets.cpp calls on one (moved here from orca-cli).
//! - [`option`]: the `ConfigOption` types a static config holds ([`ConfigOptionFloat`] and the
//!   rest) and [`ConfigOption`], the virtual interface, with the C++'s `hash()` and both of its
//!   `operator==`.
//! - [`print_config`]: the static print configs ([`PrintObjectConfig`], [`PrintRegionConfig`],
//!   [`MachineEnvelopeConfig`], [`GCodeConfig`], [`PrintConfig`], [`FullPrintConfig`] and the SLA
//!   classes) and the enums of PrintConfig.hpp, generated from the header by
//!   `gen/print-config.ts` and checked against it by `tests/static_header.rs`.
//! - [`static_config`]: [`StaticPrintConfig`] (defaults, keys, options by key, the hash) and
//!   [`ConfigBase`] (`apply_only`, `equals`, `diff`, `get_abs_value`, `set_deserialize_strict`),
//!   [`full_print_config`] and [`new_from_defaults_keys`].
//! - [`functions`]: `get_extruder_ams_count`, `compute_filament_override_value` and `has_skirt`.
//! - [`material_type`]: `MaterialType`, the table of filament types and its lookups.

// The crate's own code (config.rs, moved from orca-cli) names it as orca_config.
extern crate self as orca_config;

pub mod config;
#[rustfmt::skip]
mod def_generated;
pub mod functions;
pub mod legacy;
pub mod material_type;
pub mod option;
#[rustfmt::skip]
#[allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    clippy::upper_case_acronyms,
    clippy::enum_variant_names
)]
pub mod print_config;
pub mod static_config;
pub mod value;

pub use def_generated::{
    FILAMENT_OPTIONS, LEGACY_KEYS, PRINT_OPTIONS, PRINTER_OPTIONS, PROBE_VALUES,
};
pub use legacy::handle_legacy;
pub use option::{
    ConfigEnum, ConfigOption, ConfigOptionBool, ConfigOptionBools, ConfigOptionBoolsNullable,
    ConfigOptionEnum, ConfigOptionEnumsGeneric, ConfigOptionEnumsGenericNullable,
    ConfigOptionFloat, ConfigOptionFloatOrPercent, ConfigOptionFloats, ConfigOptionFloatsNullable,
    ConfigOptionInt, ConfigOptionInts, ConfigOptionIntsNullable, ConfigOptionPercent,
    ConfigOptionPercents, ConfigOptionPercentsNullable, ConfigOptionPoint, ConfigOptionPoints,
    ConfigOptionPointsGroups, ConfigOptionString, ConfigOptionStrings,
};
pub use print_config::{
    FullPrintConfig, GCodeConfig, MachineEnvelopeConfig, PrintConfig, PrintObjectConfig,
    PrintRegionConfig, SLAFullPrintConfig, SLAMaterialConfig, SLAPrintConfig, SLAPrintObjectConfig,
    SLAPrinterConfig,
};
pub use static_config::{ConfigBase, StaticPrintConfig, full_print_config, new_from_defaults_keys};
pub use value::{ConfigError, OptionValue};

/// `ConfigOptionType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OptionType {
    /// `coNone`.
    None,
    /// `coFloat`.
    Float,
    /// `coFloats`.
    Floats,
    /// `coInt`.
    Int,
    /// `coInts`.
    Ints,
    /// `coString`.
    String,
    /// `coStrings`.
    Strings,
    /// `coPercent`.
    Percent,
    /// `coPercents`.
    Percents,
    /// `coFloatOrPercent`.
    FloatOrPercent,
    /// `coFloatsOrPercents`.
    FloatsOrPercents,
    /// `coPoint`.
    Point,
    /// `coPoints`.
    Points,
    /// `coPoint3`.
    Point3,
    /// `coBool`.
    Bool,
    /// `coBools`.
    Bools,
    /// `coEnum`.
    Enum,
    /// `coEnums`.
    Enums,
    /// `coPointsGroups`.
    PointsGroups,
    /// `coIntsGroups`.
    IntsGroups,
}

impl OptionType {
    /// The value of the C++ enum (`coVectorType` is 0x4000).
    #[must_use]
    pub fn code(self) -> u32 {
        const V: u32 = 0x4000;
        match self {
            OptionType::None => 0,
            OptionType::Float => 1,
            OptionType::Floats => 1 + V,
            OptionType::Int => 2,
            OptionType::Ints => 2 + V,
            OptionType::String => 3,
            OptionType::Strings => 3 + V,
            OptionType::Percent => 4,
            OptionType::Percents => 4 + V,
            OptionType::FloatOrPercent => 5,
            OptionType::FloatsOrPercents => 5 + V,
            OptionType::Point => 6,
            OptionType::Points => 6 + V,
            OptionType::Point3 => 7,
            OptionType::Bool => 8,
            OptionType::Bools => 8 + V,
            OptionType::Enum => 9,
            OptionType::Enums => 9 + V,
            OptionType::PointsGroups => 10 + V,
            OptionType::IntsGroups => 11 + V,
        }
    }

    /// Whether the type is a vector (`coVectorType` set).
    #[must_use]
    pub fn is_vector(self) -> bool {
        self.code() & 0x4000 != 0
    }
}

/// `ConfigOptionMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    /// `comSimple`.
    Simple,
    /// `comAdvanced`.
    Advanced,
    /// `comExpert`.
    Expert,
    /// `comDevelop`.
    Develop,
}

/// `ConfigOptionDef::GUIType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GuiType {
    /// `undefined`.
    Undefined,
    /// `i_enum_open`.
    IEnumOpen,
    /// `f_enum_open`.
    FEnumOpen,
    /// `color`.
    Color,
    /// `select_open`.
    SelectOpen,
    /// `slider`.
    Slider,
    /// `legend`.
    Legend,
    /// `one_string`.
    OneString,
}

/// `PrinterTechnology`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PrinterTechnology {
    /// `ptFFF`.
    Fff,
    /// `ptSLA`.
    Sla,
    /// `ptUnknown`.
    Unknown,
    /// `ptAny`.
    Any,
}

/// `ConfigOptionDef`: what Orca defines for one option.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OptionDef {
    /// The key `print_config_def.options` holds it under.
    pub key: &'static str,
    /// `opt_key`.
    pub opt_key: &'static str,
    /// `type`.
    pub ty: OptionType,
    /// `nullable`: a vector that takes "nil".
    pub nullable: bool,
    /// `printer_technology`.
    pub printer_technology: PrinterTechnology,
    /// `category`.
    pub category: &'static str,
    /// `label`.
    pub label: &'static str,
    /// `full_label`.
    pub full_label: &'static str,
    /// `tooltip`.
    pub tooltip: &'static str,
    /// `sidetext`.
    pub sidetext: &'static str,
    /// `cli`.
    pub cli: &'static str,
    /// `cli_params`.
    pub cli_params: &'static str,
    /// `ratio_over`.
    pub ratio_over: &'static str,
    /// `multiline`.
    pub multiline: bool,
    /// `full_width`.
    pub full_width: bool,
    /// `is_code`.
    pub is_code: bool,
    /// `readonly`.
    pub readonly: bool,
    /// `height`.
    pub height: i32,
    /// `width`.
    pub width: i32,
    /// `min` (a `float`).
    pub min: f32,
    /// `max` (a `float`).
    pub max: f32,
    /// `max_literal`.
    pub max_literal: f64,
    /// `mode`.
    pub mode: Mode,
    /// `gui_type`.
    pub gui_type: GuiType,
    /// `gui_flags`.
    pub gui_flags: &'static str,
    /// `aliases`.
    pub aliases: &'static [&'static str],
    /// `shortcut`.
    pub shortcut: &'static [&'static str],
    /// `enum_values`.
    pub enum_values: &'static [&'static str],
    /// `enum_labels`.
    pub enum_labels: &'static [&'static str],
    /// `enum_keys_map`, in its order (`std::map`, by name).
    pub enum_keys_map: Option<&'static [(&'static str, i32)]>,
    /// `serialization_key_ordinal`.
    pub serialization_key_ordinal: u64,
    /// The type of `default_value`, when there is one.
    pub default_type: Option<OptionType>,
    /// `default_value->serialize()`, when there is a default.
    pub default: Option<&'static str>,
}

impl OptionDef {
    /// A `ConfigOptionDef` as it is constructed (Config.hpp's member initialisers): what the
    /// generated definitions leave out.
    pub const EMPTY: OptionDef = OptionDef {
        key: "",
        opt_key: "",
        ty: OptionType::None,
        nullable: false,
        printer_technology: PrinterTechnology::Fff,
        category: "",
        label: "",
        full_label: "",
        tooltip: "",
        sidetext: "",
        cli: "",
        cli_params: "",
        ratio_over: "",
        multiline: false,
        full_width: false,
        is_code: false,
        readonly: false,
        height: -1,
        width: -1,
        min: -f32::MAX,
        max: f32::MAX,
        max_literal: 1.0,
        mode: Mode::Simple,
        gui_type: GuiType::Undefined,
        gui_flags: "",
        aliases: &[],
        shortcut: &[],
        enum_values: &[],
        enum_labels: &[],
        enum_keys_map: None,
        serialization_key_ordinal: 0,
        default_type: None,
        default: None,
    };

    /// `default_value`: the option's default, read from its serialisation into a value of its
    /// default's type, or `None` when it has no default. The probe records a default as its
    /// serialisation, so a double default holds the six significant digits `serialize` writes
    /// (`parity/HAZARDS.md`).
    ///
    /// # Panics
    ///
    /// If the generated default does not read back, which `tests/def.rs` rules out.
    #[must_use]
    pub fn default_value(&self) -> Option<OptionValue> {
        let text = self.default?;
        let ty = self.default_type.unwrap_or(self.ty);
        let mut v = OptionValue::empty(ty, self.nullable, self.enum_keys_map);
        match v.deserialize(text) {
            Ok(true) => Some(v),
            _ => panic!("{}: its default {text:?} does not read back", self.key),
        }
    }

    /// `create_empty_option()`: a value of the option's type as its constructor makes it.
    #[must_use]
    pub fn empty_value(&self) -> OptionValue {
        OptionValue::empty(self.ty, self.nullable, self.enum_keys_map)
    }
}

/// `print_config_def.options`, in its order (by key, byte for byte).
#[must_use]
pub fn options() -> &'static [OptionDef] {
    def_generated::OPTIONS
}

/// `print_config_def.get(key)`.
#[must_use]
pub fn get(key: &str) -> Option<&'static OptionDef> {
    let all = def_generated::OPTIONS;
    let i = orca_stl::lower_bound_by(all, key, |d, key: &str| d.key.as_bytes() < key.as_bytes());
    all.get(i).filter(|d| d.key == key)
}

/// `print_config_def.has(key)`.
#[must_use]
pub fn has(key: &str) -> bool {
    get(key).is_some()
}
