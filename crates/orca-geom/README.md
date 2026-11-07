# orca-geom

Clipper 6.4.2, ClipperUtils and libslic3r's geometry: points, polygons, ExPolygons, boxes, hulls.

The geometry everything after the model's reading stands on: Clipper 6.4.2 as OrcaSlicer vendors it (`deps_src/clipper`, compiled with `Slic3r::Point` as its `IntPoint`), with `Int128` as `i128`; libslic3r's `ClipperUtils`; and libslic3r's geometry: `Point`, `Polygon`, `Polyline`, `Line`, `MultiPoint` (Douglas-Peucker and Visvalingam), `ExPolygon` (without the medial axis), `BoundingBox`, the convex hulls of `Geometry.cpp` and `Geometry/ConvexHull.cpp`, `Geometry::Transformation`, and `scale_` and `unscale` with the command line's `SCALING_FACTOR` of 1e-6. `coord_t` is `i64`.

## Modules

- `clipper`: Clipper 6.4.2 whole (`deps_src/clipper/clipper.cpp`). `engine.rs` is `ClipperBase` and `Clipper`: edges, local minima, the scanbeam, the active and sorted edge lists, intersections, horizontals, joins, and the output records built into `Paths` or a `PolyTree`. `offset.rs` is `ClipperOffset`; `polytree.rs` is `PolyNode` and `PolyTree`, the nodes in one vector linked by index; `mod.rs` holds the types and the free functions (`Area`, `PointInPolygon`, `SimplifyPolygon(s)`, `CleanPolygon(s)`, `Minkowski*`, the `PolyTree` conversions). The C++ pointers are indices into arenas, allocated in the C++ order. Sorts and the scanbeam's `std::priority_queue` go through `orca-stl` (MSVC's introsort and heap, so ties fall the same way), `acos`, `sin`, `cos`, `atan2` and the float-to-integer casts through `orca-crt`. An exception Clipper throws to its caller (`RangeTest`, an open clip path, `Execute` to `Paths` after an open path) is a panic whose payload is a `ClipperException`, which `catch_unwind` and `downcast` catch as `catch (clipperException&)` does; the two it catches itself (`UpdateEdgeIntoAEL`, `DoMaxima`) are a `Result` inside. The `ClipperLib_Z` sites are marked `ClipperLib_Z` in `engine.rs`, for when the Z variant is needed; Clipper2 is not here.
- `int128`: `Int128.hpp`'s filtered predicates. MSVC compiles its `_mul128` branch, whose exact products are `i128`; the estimate adds `1 << 31`, which MSVC makes `INT_MIN`, so it subtracts half a word.
- `point`: `coord_t` (`i64`) and `Point`.

## Probe gate

`tests/probes.rs` replays the records of `geom.sop1` but the Eigen ones, which orca-eigen replays (`parity/SOP1.md`, "geom.sop1"): CCLP (Clipper runs), COFF (offsets), CARE (`Area`, `Orientation`, `PointInPolygon`), CCLN (`CleanPolygon`), CSIM (`SimplifyPolygon(s)`), CMNK (Minkowski) and CINT (`Int128`). `bun run engine parity probe --only geom` writes the file from the C++ engine's own build (`parity/oracle/probe/geom/clipper.cpp`): the committed subset in `parity/probes/` runs by default, the full set with `SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes`. A record kind nothing replays fails the test.

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
