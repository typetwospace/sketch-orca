# orca-qhull

Qhull 2015.2 (libqhull_r 7.2.0 and libqhullcpp), as OrcaSlicer builds it from deps_src/qhull.

Qhull 2015.2 (`libqhull_r` 7.2.0 and `libqhullcpp`) as the C++ engine links it, built from OrcaSlicer's `deps_src/qhull` as `qhullstatic`: the convex hulls `TriangleMesh` and the model take, with Qhull's merging and its `qsort` calls (`merge_r.c`, `mem_r.c`, `io_r.c`), which go through `orca-crt`'s `qsort`, the static UCRT's own.

The o2-mesh task of phase O2 fills this crate; its records are in the probe file `qhull.sop1`, which `tests/probes.rs` replays.

## Probe gate

`tests/probes.rs` replays the records of `qhull.sop1` (`parity/SOP1.md`, "Probe files"), which `bun run engine parity probe --only qhull` writes from the C++ engine's own build: the committed subset in `parity/probes/` by default, the full set with `SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes`. While a file is a stub (HEAD and PRUN only) the test passes trivially; a file with records and no replay fails it.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only; the code
it carries keeps its own licence, which `NOTICE` names in the crate's own section.

## Dependencies

Declared up front, before the crate is filled (o2-geom, 2026-11-07), so that the tasks that fill the O2 crates in parallel never change `Cargo.toml` or `Cargo.lock`:

- `orca-crt`
- `orca-stl`
- `orca-fmt`
- `orca-probe` (development only: the probe gate)
