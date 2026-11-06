# orca-fmt

printf, to_chars, iostream double output and strtod, stod and from_chars parsing, as the C++ engine gets them.

Number formatting and parsing that gives the digits the C++ engine's runtime gives: `printf` written from the C standard, the standard library's shortest round-trip `to_chars` (Ryu), the default and fixed `iostream` double output, and `strtod`, `stod` and `from_chars` parsing.

Each function is tested against the C++ oracle through the probes (`orca-probe`) and against the UCRT itself.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.

## Rules

- Engine crates format and parse numbers only through this crate; Rust's `format!("{}")` of a float is not `printf`.

## Dependencies

None.
