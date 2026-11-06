# orca-config

Orca's configuration: the definition of every option, the values options hold, and the renames and drops Orca applies to the keys it reads (OrcaSlicer v2.4.2, commit 8500fcd).

- `options()`, `get(key)`, `has(key)`: `print_config_def`, every one of Orca's 751 options with every field of its `ConfigOptionDef` (`OptionDef`: type, nullable, printer technology, labels, tooltip, limits as `float`s, mode, GUI type and flags, aliases, enum values, labels and keys map, the default), in its order (by key). `PRINT_OPTIONS`, `FILAMENT_OPTIONS` and `PRINTER_OPTIONS` are `Preset::print_options()` and the others, the keys of each preset section; `LEGACY_KEYS` are the keys `handle_legacy` names.
- `src/def_generated.rs` holds them, generated and never typed in: `bun scripts/orca.ts parity gen-config` writes it from the configdef probe (`parity/probes/configdef.json`, which `parity probe` records from Orca's built library), and `parity gen-config --check` exits 1 while the file differs from what the probe gives (`parity/README.md`).
- `OptionValue` (`src/value.rs`): the values of Orca's `ConfigOption` types, each with its `serialize` and `deserialize` as `Config.hpp` writes them: `ostream` output at precision 6 for doubles (orca-fmt), `istream >>` and `sscanf` for reading them, `nil` for a nullable vector's empty element (NaN, `INT_MAX` or 255), points as `x` and `y` separated by `x` or `,`, strings with `escape_string_cstyle`, and enums by name through the option's keys map. `OptionDef::default_value()` reads the option's default back from its serialisation; `empty_value()` is `create_empty_option()`.
- `handle_legacy(opt_key, value)` (`src/legacy.rs`): `PrintConfigDef::handle_legacy`, branch for branch and in its order: the keys Orca renames, the values it rewrites, and the keys it drops (an empty key).

The command line's rules for preset keys build on these: a key is a setting when `has(key)` is true after `handle_legacy`; `PRINT_OPTIONS` and the others say which preset section takes it.

Each file that carries OrcaSlicer code says which code from which file in its `Carries OrcaSlicer code` block; the generated file says it in its header.

`tests/def.rs` holds the crate to the probe: every field of every definition equals what the probe read from the built library; every default reads back and writes the same text; `handle_legacy` does what Orca's did for each of the probe's 2042 recorded cases (every legacy and defined key with each of its 22 values) and changes nothing in every other case; and `different_settings_to_system`'s splitting of a preset's list. `src/value.rs` and `src/legacy.rs` hold their own unit tests (the edges of each type's reading, `nil`, escapes, points).

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.

## Rules

- A setting is stored with the type the C++ gives it: `f32` where it is a `float`.
- Never edit `src/def_generated.rs`: change the probe or `scripts/lib/parity/gen-config.ts` and run `parity gen-config`.

## Dependencies

- `orca-json` (the probe, in the tests)
- `orca-fmt` (`ostream` output and `istream` input of numbers)
- `orca-crt`
- `orca-stl` (`lower_bound`, the search of the definitions)
