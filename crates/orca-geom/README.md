# orca-geom

Clipper 6.4.2, ClipperUtils and libslic3r's geometry: points, polygons, ExPolygons, boxes, hulls.

The geometry everything after the model's reading stands on: Clipper 6.4.2 as OrcaSlicer vendors it (`deps_src/clipper`, compiled with `Slic3r::Point` as its `IntPoint`), with `Int128` as `i128`; libslic3r's `ClipperUtils`; and libslic3r's geometry: `Point`, `Polygon`, `Polyline`, `Line`, `MultiPoint` (Douglas-Peucker; Visvalingam is left out, as nothing calls it and its code is undefined behaviour), `ExPolygon` (without the medial axis), `BoundingBox`, the predicates, angles, transform builders and convex hulls of `Geometry.cpp` and `Geometry/ConvexHull.cpp`, and `scale_` and `unscale` with the command line's `SCALING_FACTOR` of 1e-6. `coord_t` is `i64`. Not here yet: `Geometry::Transformation` and `TransformationSVD`, `Geometry::arrange`, the arc fitting of `Polyline`, `Polyline3` and `ThickPolyline`, and the ClipperUtils overloads on `Surface`s and the chained traversals that need ShortestPath (`parity/HAZARDS.md`, orca-geom, lists them).

## Modules

- `clipper`: Clipper 6.4.2 whole (`deps_src/clipper/clipper.cpp`). `engine.rs` is `ClipperBase` and `Clipper`: edges, local minima, the scanbeam, the active and sorted edge lists, intersections, horizontals, joins, and the output records built into `Paths` or a `PolyTree`. `offset.rs` is `ClipperOffset`; `polytree.rs` is `PolyNode` and `PolyTree`, the nodes in one vector linked by index; `mod.rs` holds the types and the free functions (`Area`, `PointInPolygon`, `SimplifyPolygon(s)`, `CleanPolygon(s)`, `Minkowski*`, the `PolyTree` conversions). The C++ pointers are indices into arenas, allocated in the C++ order. Sorts and the scanbeam's `std::priority_queue` go through `orca-stl` (MSVC's introsort and heap, so ties fall the same way), `acos`, `sin`, `cos`, `atan2` and the float-to-integer casts through `orca-crt`. An exception Clipper throws to its caller (`RangeTest`, an open clip path, `Execute` to `Paths` after an open path) is a panic whose payload is a `ClipperException`, which `catch_unwind` and `downcast` catch as `catch (clipperException&)` does; the two it catches itself (`UpdateEdgeIntoAEL`, `DoMaxima`) are a `Result` inside. The `ClipperLib_Z` sites are marked `ClipperLib_Z` in `engine.rs`, for when the Z variant is needed; Clipper2 is not here.
- `int128`: `Int128.hpp`'s filtered predicates. MSVC compiles its `_mul128` branch, whose exact products are `i128`; the estimate adds `1 << 31`, which MSVC makes `INT_MIN`, so it subtracts half a word.
- `point`: `coord_t` (`i64`), `Point` (`Eigen::Matrix<coord_t, 2, 1, DontAlign>`), `scale_`, `unscale` and the point helpers of `Point.hpp`. Its arithmetic wraps, as MSVC's does. `cwiseMin` and `cwiseMax` are Eigen's SSE2 `pmin` and `pmax` of a `Packet2l`, whose 64-bit compare takes the low 32-bit halves signed (`orca_eigen::pcmp_lt_i64`): past 2^31 they can pick the wrong coordinate, and a `BoundingBox` then misses a point, as the engine's does.
- `line`: `Line` and `line_alg`'s distances, intersections and angles.
- `multi_point`: `MultiPoint` as functions of a point slice (the C++ class's virtual `lines()` becomes an argument), with `_douglas_peucker` and `get_extents_rotated`.
- `polygon`, `polyline`, `expolygon`: `Polygon`, `Polyline` and `ExPolygon` with their free helpers (`remove_sticks`, `remove_collinear`, `polygons_simplify`, `make_circle`, `foot_pt`, `keep_largest_contour_only`, the `to_*` conversions and `get_extents*`). C++ out-parameters that report success become `Option`s (`Polyline::split_at_length`).
- `bounding_box`: `BoundingBox`, `BoundingBoxf` and `BoundingBoxf3` with `transformed`.
- `clipper_utils`: `ClipperUtils.cpp`: the offsets (`offset`, `offset_ex`, `offset2_ex`, `closing`, `opening`, `expand`, `shrink`), the boolean operations on polygons, ExPolygons, polylines and lines, `union_safety_offset`, `simplify_polygons`, `top_level_islands`, `fix_after_*_offset`, the variable-width offsets (`mittered_offset_path_scaled`, `variable_offset_*`) and the bounding box clip of `ClipperUtils::clip_clipper_polygons_with_subject_bbox`. The C++ overloads are functions named for their argument types (`diff_ex_expolygons`, `intersection_pl_polygons`); a path provider (`PathsProvider`, `ExPolygonsProvider`) is a list of point slices, `PathsRef`, an ExPolygon giving its contour, then its holes.
- `geometry`: `Geometry.hpp` and `Geometry.cpp`: `orient`, the segment and ray intersections, `foot_pt`, the Liang-Barsky clip, `directions_parallel`, the angle folds, `assemble_transform`, `rotation_transform`, `scale_transform` and `extract_euler_angles`.
- `convex_hull`: `Geometry/ConvexHull.cpp`: the convex hull of points and the one of `Vec3d`s in x and y (`convex_hull_3d`), `convex_polygons_intersect` (rotating calipers) and the top and bottom chains of a convex polygon with `inside_convex_polygon`.

Sorts, heaps, selections and float min and max go through `orca-stl`, maths and float-to-integer casts through `orca-crt`, the vectors and transforms through `orca-eigen`, with `f32` where the C++ has `float` (the ClipperUtils deltas) and the C++ order of evaluation.

## Probe gate

`tests/probes.rs` replays the records of `geom.sop1` but the Eigen ones, which orca-eigen replays (`parity/SOP1.md`, "geom.sop1"): CCLP (Clipper runs), COFF (offsets), CARE (`Area`, `Orientation`, `PointInPolygon`), CCLN (`CleanPolygon`), CSIM (`SimplifyPolygon(s)`), CMNK (Minkowski), CINT (`Int128`), GPNT (`Point`, `Line` and the `Geometry` predicates), GPLY (`Polygon`, `Polyline`, `MultiPoint`, `BoundingBox`, the 2D hull), GCLU (ClipperUtils and `ExPolygon`) and GXFM (the transform builders, `BoundingBoxf3::transformed`, the hull of `Vec3d`s, `make_circle`, `convex_polygons_intersect`, the convex chains). Every result is compared bit for bit, a `double` by its bits. `bun run engine parity probe --only geom` writes the file from the C++ engine's own build (`parity/oracle/probe/geom/clipper.cpp` and `geometry.cpp`): the committed subset in `parity/probes/` runs by default, the full set with `SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes`. A record kind nothing replays fails the test.

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
