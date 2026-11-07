# orca-exact

Exact arithmetic as Boost 1.84.0 has it: multiprecision's signed-magnitude int128_t and rational.

Exact integer and rational arithmetic as the C++ engine gets it from Boost 1.84.0: Boost.Multiprecision's `int128_t` (a signed-magnitude `cpp_int` of 128 bits, which wraps on overflow otherwise than Rust's `i128`) and `boost::rational`, which libnest2d and `Arrange.cpp` compute with (`<work>/deps/OrcaSlicer_dep/usr/local/include/boost-1_84`).

The o2-arrange task of phase O2 fills this crate; its records go into the probe file `arrange.sop1` beside those of `orca-arrange`, and `tests/probes.rs` replays them.

## Probe gate

`tests/probes.rs` replays the records of `arrange.sop1` (`parity/SOP1.md`, "Probe files"), which `bun run engine parity probe --only <name>` writes from the C++ engine's own build: the committed subset in `parity/probes/` by default, the full set with `SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes`. While a file is a stub (HEAD and PRUN only) the test passes trivially; a file with records and no replay fails it.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only; the code
it carries keeps its own licence, which `NOTICE` names in the crate's own section.

## Dependencies

Declared up front, before the crate is filled (o2-geom, 2026-11-07), so that the tasks that fill the O2 crates in parallel never change `Cargo.toml` or `Cargo.lock`:

- `orca-crt`
- `orca-stl`
- `orca-probe` (development only: the probe gate)
