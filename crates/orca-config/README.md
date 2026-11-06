# orca-config

A scaffold: the crate holds only its name until task O1.8b of the translation's plan fills it.

It is to hold Orca's configuration: `ConfigDef` (generated from the `config-def` probe, not typed in by hand), the legacy and obsolete keys Orca renames or drops when it reads a configuration, and the serialisation of a configuration.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.

## Rules

- A setting is stored with the type the C++ gives it: `f32` where it is a `float`.

## Dependencies

- `orca-json`
- `orca-fmt`
- `orca-crt`
- `orca-stl`
