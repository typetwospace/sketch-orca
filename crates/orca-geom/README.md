# orca-geom

Clipper 6.4.2, ClipperUtils and libslic3r's geometry: points, polygons, ExPolygons, boxes, hulls.

The geometry everything after the model's reading stands on: Clipper 6.4.2 as OrcaSlicer vendors it (`deps_src/clipper`, compiled with `Slic3r::Point` as its `IntPoint`), with `Int128` as `i128`; libslic3r's `ClipperUtils`; and libslic3r's geometry: `Point`, `Polygon`, `Polyline`, `Line`, `MultiPoint` (Douglas-Peucker and Visvalingam), `ExPolygon` (without the medial axis), `BoundingBox`, the convex hulls of `Geometry.cpp` and `Geometry/ConvexHull.cpp`, `Geometry::Transformation`, and `scale_` and `unscale` with the command line's `SCALING_FACTOR` of 1e-6. `coord_t` is `i64`.

The o2-geom task of phase O2 fills this crate; its records are in the probe file `geom.sop1`, which `tests/probes.rs` replays.

## Probe gate

`tests/probes.rs` replays the records of `geom.sop1` (`parity/SOP1.md`, "Probe files"), which `bun run engine parity probe --only <name>` writes from the C++ engine's own build: the committed subset in `parity/probes/` by default, the full set with `SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes`. While a file is a stub (HEAD and PRUN only) the test passes trivially; a file with records and no replay fails it.

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
- `orca-probe` (development only: the probe gate)
