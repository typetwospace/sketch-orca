# orca-config

Orca's configuration: the definition of every option, the values options hold, the renames and drops Orca applies to the keys it reads, the dynamic config the presets stage builds, and the static print configs the slicing stages read (OrcaSlicer v2.4.2, commit 8500fcd).

- `options()`, `get(key)`, `has(key)`: `print_config_def`, every one of Orca's 751 options with every field of its `ConfigOptionDef` (`OptionDef`: type, nullable, printer technology, labels, tooltip, limits as `float`s, mode, GUI type and flags, aliases, enum values, labels and keys map, the default), in its order (by key). `PRINT_OPTIONS`, `FILAMENT_OPTIONS` and `PRINTER_OPTIONS` are `Preset::print_options()` and the others, the keys of each preset section; `LEGACY_KEYS` are the keys `handle_legacy` names.
- `src/def_generated.rs` holds them, generated and never typed in: `bun scripts/orca.ts parity gen-config` writes it from the configdef probe (`parity/probes/configdef.json`, which `parity probe` records from Orca's built library), and `parity gen-config --check` exits 1 while the file differs from what the probe gives (`parity/README.md`).
- `OptionValue` (`src/value.rs`): the values of Orca's `ConfigOption` types, each with its `serialize` and `deserialize` as `Config.hpp` writes them: `ostream` output at precision 6 for doubles (orca-fmt), `istream >>` and `sscanf` for reading them, `nil` for a nullable vector's empty element (NaN, `INT_MAX` or 255), points as `x` and `y` separated by `x` or `,`, strings with `escape_string_cstyle`, and enums by name through the option's keys map. `OptionDef::default_value()` reads the option's default back from its serialisation; `empty_value()` is `create_empty_option()`.
- `handle_legacy(opt_key, value)` (`src/legacy.rs`): `PrintConfigDef::handle_legacy`, branch for branch and in its order: the keys Orca renames, the values it rewrites, and the keys it drops (an empty key).
- `config::Config` (`src/config.rs`): the `DynamicPrintConfig` the presets stage builds, and what cli/presets.cpp calls on one (`set_deserialize` with substitutions, `normalize_fdm`, `validate`, the extruder variants). It moved here from orca-cli on 2026-11-13, unchanged, so the crates below orca-cli can use it; orca-cli's `src/config.rs` re-exports it. orca-model's `OptionSource` and orca-placeholder's `Symbols` are implemented for it in those crates.
- `option` (`src/option.rs`): one type per `ConfigOption` class a static config holds (`ConfigOptionFloat`, `ConfigOptionFloatsNullable`, `ConfigOptionEnum<T>` and the rest) and `ConfigOption`, the virtual interface. Each type's `PartialEq` is its own `operator==` (a float and a float-or-percent within 1e-4, a percent exactly, two nils equal in a nullable vector of doubles); `ConfigOption::eq_value` is the virtual one (where a percent compares within 1e-4 too). `hash()` is MSVC's `std::hash` (orca-stl) of the value, folded with Boost 1.84's `hash_combine` for a vector, as the C++ hashes. `apply_override` is `ConfigOptionVector::apply_override`.
- `print_config` (`src/print_config.rs`): the static print configs (`PrintObjectConfig`, `PrintRegionConfig`, `MachineEnvelopeConfig`, `GCodeConfig`, `PrintConfig`, `FullPrintConfig`, `SLAPrintConfig`, `SLAPrintObjectConfig`, `SLAMaterialConfig`, `SLAPrinterConfig`, `SLAFullPrintConfig`) and the 51 enums of PrintConfig.hpp. Generated, never typed in: `bun crates/orca-config/gen/print-config.ts` writes it from the work folder's `src/src/libslic3r/PrintConfig.hpp` (`--check` exits 1 while it differs). A class holds each parent as a field (`print.gcode`), first, and its own options in the header's order, since that order decides its hash.
- `static_config` (`src/static_config.rs`): `StaticPrintConfig` (a class's keys in the definition's order, its defaults from the definition, its options by key, its hash, `to_dynamic`) and `ConfigBase`, for every class and for `Config`: `apply_only` (with `key#n`), `apply_config`, `equals`, `diff`, `equal`, `opt_serialize`, both `get_abs_value`, `set_deserialize_strict` and the two `set`s. `full_print_config()` and `new_from_defaults_keys(keys)` are DynamicPrintConfig's.
- `functions` (`src/functions.rs`): `get_extruder_ams_count`, `compute_filament_override_value` and `has_skirt` of PrintConfig.cpp.
- `material_type` (`src/material_type.rs`): `MaterialType`, Orca's table of 75 filament types (temperatures, chamber, adhesion, yield strength, thermal length) and its lookups by exact name.

The command line's rules for preset keys build on these: a key is a setting when `has(key)` is true after `handle_legacy`; `PRINT_OPTIONS` and the others say which preset section takes it.

### Where the C++ leaves defined behaviour

These stop with a `ConfigError` that says what the C++ would have done; the probe never reaches them.

- `ConfigBase::apply_only` of a `key#n` whose `key` this config cannot hold as a vector while the source has it (a null option dereferenced), and `set_deserialize_strict` of a key a static class lacks.
- `get_abs_value` of a percent whose `ratio_over` chain ends at another type, of an option of no numeric type, or of a key with no definition.
- A static `ConfigOptionEnumsGeneric` has no names (its `keys_map` stays null): it serializes each value as "" and refuses to read a name.
- `get_extruder_ams_count` of a part with no `#`; `compute_filament_override_value` with no int `enable_long_retraction_when_cut`, or a retraction filament of another type.
- `ConfigOptionEnum<T>` given a value its enum does not declare (the C++ casts it).

Not translated here (no piece of domain A reaches them): the `FloatsOrPercents`, `Point3` and `IntsGroups` option classes as static types, `operator<`, `vserialize`, `set_with_*` beyond the moved `Config`'s, and the bed's excluded area (orca-o3-front-2).

Each file that carries OrcaSlicer code says which code from which file in its `Carries OrcaSlicer code` block; the generated file says it in its header.

`tests/def.rs` holds the crate to the probe: every field of every definition equals what the probe read from the built library; every default reads back and writes the same text; `handle_legacy` does what Orca's did for each of the probe's 2042 recorded cases (every legacy and defined key with each of its 22 values) and changes nothing in every other case; and `different_settings_to_system`'s splitting of a preset's list. `src/value.rs`, `src/legacy.rs` and `src/config.rs` hold their own unit tests (the edges of each type's reading, `nil`, escapes, points, the presets stage's config).

`tests/static_header.rs` reads the work folder's PrintConfig.hpp with a reader of its own and fails while `src/print_config.rs` differs from it: every enum (name, size, values), every class (parents and fields in order), each class's flattened fields, its table by key, and each option's type and nullability. It fails naming the header's path when the work folder lacks it.

`tests/probes.rs` replays the config records of `layer.sop1` (`parity/sop1/layer.md`; the committed subset, or the full set when `SKETCH_ORCA_PROBES_DEEP` names its folder): each class's defaults, keys, types, serialisations and hash (LYCK), `get_abs_value` (LYCA), `set_deserialize_strict` then the hash, `==`, `equals`, `diff` and `equal` (LYCD), `apply_only` (LYCO), `full_print_config` and `new_from_defaults_keys` (LYDY), `get_extruder_ams_count` (LYAM), `compute_filament_override_value` (LYFO), `has_skirt` (LYSK) and `MaterialType` (LYMT). The file's other kinds are orca-layer's; any kind outside both lists, or a record count other than the probe's, fails the test. orca-config has no orca-probe dev-dependency, so the test compiles orca-probe's reader in from its source (`#[path]`), with that reader's own unit tests.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.

## Rules

- A setting is stored with the type the C++ gives it: `f32` where it is a `float`.
- Never edit `src/def_generated.rs`: change the probe or `scripts/lib/parity/gen-config.ts` and run `parity gen-config`.
- Never edit `src/print_config.rs`: change `gen/print-config.ts` (or the header) and run `bun crates/orca-config/gen/print-config.ts`.

## Dependencies

- `orca-json` (the probe, in the tests)
- `orca-fmt` (`ostream` output and `istream` input of numbers, `stoi`, `atoi`, `to_chars`)
- `orca-crt` (`fabs`)
- `orca-stl` (`lower_bound`, the search of the definitions and of a class's keys; `std::hash`, `std::min` and `std::max`)
