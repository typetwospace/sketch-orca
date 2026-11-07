# orca-xml

XML as expat 2.5.0 parses it, for 3MF files.

XML as the C++ engine parses it with expat 2.5.0, the dependency build's (`src/deps/EXPAT/expat` in OrcaSlicer's tree, not `deps_src/expat`): the callbacks a 3MF reader gets, in their order, and the errors expat reports.

The o2-model task of phase O2 fills this crate; its records are in the probe file `xml.sop1`, which `tests/probes.rs` replays.

## Probe gate

`tests/probes.rs` replays the records of `xml.sop1` (`parity/SOP1.md`, "Probe files"), which `bun run engine parity probe --only <name>` writes from the C++ engine's own build: the committed subset in `parity/probes/` by default, the full set with `SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes`. While a file is a stub (HEAD and PRUN only) the test passes trivially; a file with records and no replay fails it.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only; the code
it carries keeps its own licence, which `NOTICE` names in the crate's own section.

## Dependencies

Declared up front, before the crate is filled (o2-geom, 2026-11-07), so that the tasks that fill the O2 crates in parallel never change `Cargo.toml` or `Cargo.lock`:

- `orca-crt`
- `orca-stl`
- `orca-fmt`
- `orca-probe` (development only: the probe gate)
