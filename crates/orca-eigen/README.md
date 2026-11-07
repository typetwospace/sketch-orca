# orca-eigen

The Eigen 5.0.1 operations libslic3r's geometry reaches, evaluated as MSVC 14.42 compiles them.

The Eigen operations libslic3r's geometry reaches, evaluated as MSVC 14.42 compiles Eigen 5.0.1 with the C++ engine's flags (`/O2`, `/fp:precise`, SSE2): fixed-size vectors and matrices of `float`, `double` and `coord_t` (`Vec2`, `Vec3`, `Matrix3d`, `Matrix4d`), `Transform3d` and `Affine3d` products, inverses and `linear()`, `AngleAxis` and the rotation matrices `Geometry` builds from Euler angles, and `norm`, `squaredNorm`, `dot`, `cross`, `normalized`, the coefficient-wise operations, `minCoeff`, `maxCoeff` and the casts. Where Eigen vectorises, the SSE2 packets and their reduction order are the result (`parity/HAZARDS.md`, "Geometry kernels").

The dependency build pins Eigen 5.0.1 (`<work>/deps/dep_Eigen-prefix/src/dep_Eigen`), whose headers number it 3.5.0. The o2-geom task of phase O2 fills this crate; its records are in the probe file `geom.sop1`, which `tests/probes.rs` replays.

## Probe gate

`tests/probes.rs` replays the records of `geom.sop1` (`parity/SOP1.md`, "Probe files"), which `bun run engine parity probe --only <name>` writes from the C++ engine's own build: the committed subset in `parity/probes/` by default, the full set with `SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes`. While a file is a stub (HEAD and PRUN only) the test passes trivially; a file with records and no replay fails it.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only; the code
it carries keeps its own licence, which `NOTICE` names in the crate's own section.

## Dependencies

Declared up front, before the crate is filled (o2-geom, 2026-11-07), so that the tasks that fill the O2 crates in parallel never change `Cargo.toml` or `Cargo.lock`:

- `orca-crt`
- `orca-stl`
- `orca-probe` (development only: the probe gate)
