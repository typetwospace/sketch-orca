# orca-arrange

Arrangement: libnest2d, Arrange.cpp, NLopt 2.5.0 and Boost.Geometry's R*-tree.

Arrangement as the C++ engine places objects on the bed: libnest2d (`deps_src/libnest2d`, with libslic3r's geometry and NLopt as its optimiser), libslic3r's `Arrange.cpp`, NLopt 2.5.0's subplex and Nelder-Mead (`sbplx`, `nldrmd`, `redblack`, `stop` and the API) and Boost.Geometry's R*-tree (Boost 1.84.0's `geometry/index`).

The o2-arrange task of phase O2 fills this crate; its records are in the probe file `arrange.sop1`, which `tests/probes.rs` replays.

## Probe gate

`tests/probes.rs` replays the records of `arrange.sop1` (`parity/SOP1.md`, "Probe files"), which `bun run engine parity probe --only <name>` writes from the C++ engine's own build: the committed subset in `parity/probes/` by default, the full set with `SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes`. While a file is a stub (HEAD and PRUN only) the test passes trivially; a file with records and no replay fails it.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only; the code
it carries keeps its own licence, which `NOTICE` names in the crate's own section.

## Dependencies

Declared up front, before the crate is filled (o2-geom, 2026-11-07), so that the tasks that fill the O2 crates in parallel never change `Cargo.toml` or `Cargo.lock`:

- `orca-crt`
- `orca-stl`
- `orca-tbb`
- `orca-fmt`
- `orca-eigen`
- `orca-geom`
- `orca-exact`
- `orca-probe` (development only: the probe gate)
