# orca-mesh

Triangle meshes: admesh's reading and repair, and libslic3r's TriangleMesh with its transforms.

Triangle meshes as the C++ engine reads and repairs them: admesh (`deps_src/admesh`: the STL reader's passes, the facet connectivity, the repair steps and the statistics) and libslic3r's `TriangleMesh` (`TriangleMesh.cpp`), with its transforms, its bounding boxes and its convex hull through `orca-qhull`. admesh reads files with the C runtime's stdio (`fopen`, `fread`, `fscanf`), which `orca-fmt`'s `stdio` and `scanf` modules reproduce.

The o2-mesh task of phase O2 fills this crate; its records are in the probe file `mesh.sop1`, and those of the C runtime's stdio in `cstdio.sop1`. `tests/probes.rs` replays both until o2-mesh gives `cstdio.sop1` a test of its own.

## Probe gate

`tests/probes.rs` replays the records of `mesh.sop1` and `cstdio.sop1` (`parity/SOP1.md`, "Probe files"), which `bun run engine parity probe --only mesh` and `bun run engine parity probe --only cstdio` write from the C++ engine's own build: the committed subset in `parity/probes/` by default, the full set with `SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes`. While a file is a stub (HEAD and PRUN only) the test passes trivially; a file with records and no replay fails it.

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
- `orca-qhull`
- `orca-probe` (development only: the probe gate)
