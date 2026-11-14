# orca-fill

Phase O3 of the Rust translation: libslic3r's infill: FillBase and its helpers, gap fill and infill
connection, the rectilinear patterns, cross hatch, the concentric patterns, Fill::new_from_type,
make_fills and PrintObject::infill.

Set up empty by orca-o3-skeleton on 2026-11-13: a manifest that declares every dependency the crate
will need, so that no unit that fills it changes `Cargo.toml` or `Cargo.lock`, and a `src/lib.rs`
with the header and the crate's doc comment. Each piece below adds its modules and its tests and
keeps this README true.

## The pieces that fill it

| Piece | What it adds |
| --- | --- |
| `orca-o3-fill-1` | FillBase and its helpers (2026-11-14) |
| `orca-o3-fill-2` | Gap fill, infill connection and the rectilinear set-up (2026-11-14) |
| `orca-o3-fill-3` | The rectilinear patterns' graph and cross hatch |
| `orca-o3-fill-4` | Make_fills, the concentric patterns, Fill::new_from_type and PrintObject::infill |

The C++ they translate (under `src/libslic3r/` of OrcaSlicer v2.4.2 unless named): Fill/FillBase.hpp
and .cpp, Fill/FillRectilinear.hpp and .cpp, Fill/FillCrossHatch.hpp and .cpp, Fill/Fill.cpp,
Fill/FillConcentric.hpp and .cpp, Fill/FillConcentricInternal.hpp and .cpp, PrintObject.cpp.

## The modules

| Module | What it carries |
| --- | --- |
| `base` | `Fill/FillBase.hpp` lines 1-235 and `Fill/FillBase.cpp` lines 1-39, 105-132, 255-321 and 1243-1262 (fill-1): `FillParams` with its defaults, `LockRegionParam`, `class Fill` as the trait `Fill` over the fields of `FillBase` (each virtual method a trait method whose default body is also a free function, `fill_surface_base` and the like, so an overriding filler can still call the base), the anchors' statics as constants, `fill_surface`, `fill_surface_arachne`, `_layer_angle`, `_infill_direction`, `_adjust_solid_spacing` (`adjust_solid_spacing`) and the two `connect_infill` forms that hand on to the third. What a filler throws is `FillError`: `InfillFailed`, or orca-layer's `FlowError`. |
| `contour` | `Fill/FillBase.cpp` lines 322-1242 and 1263-1264 (fill-1), what `connect_infill` builds on: `ContourIntersectionPoint` (its neighbours on the contour are indices into the vector that holds every T-joint, the C++'s pointers), the distances along a closed contour, the walks that take a piece of contour onto a line (`take_cw_full`, `take_ccw_full`, `take_cw_limited`, `take_ccw_limited`, both `take` forms and `take_limited`), the ends of a line clipped by length, `line_rounded_thick_segment_collision` and `mark_boundary_segments_touching_infill`. It also holds `line_alg::distance_to_squared` of a `Linef` and `BoundingBoxf::overlap`, which belong in orca-geom; no piece carries them there. |
| `extrusion` | `Fill/FillBase.cpp` lines 133-254 (fill-2): `Fill::fill_surface_extrusion`'s base body (`fill_surface_extrusion_base`, the trait method's default), which turns the filler's lines into one collection of extrusions with the flow worked out again from the filler's spacing, and `_create_gap_fill` (`create_gap_fill`), the gaps the collection leaves in the no-overlap islands filled by the medial axis with gap fill of variable width, as the print object's gap fill target allows. `InfillFailedException` is caught; the flow's errors and the medial axis's exceptions go on up. |
| `connect` | `Fill/FillBase.cpp` lines 1265-1833 (fill-2): `BoundaryInfillGraph` and `create_boundary_infill_graph` (the lines' ends projected onto the boundary through an `EdgeGrid`, each contour a ring of T-joints and its points), the third `connect_infill` (the form the other two hand on to: neighbouring lines joined along the boundary while the arc is short enough, then the ends hooked onto the boundary by the anchors), `chain_or_connect_infill` and `Fill::extended_object_bounding_box`. |
| `rectilinear` | `Fill/FillRectilinear.hpp` lines 1-232 and `Fill/FillRectilinear.cpp` lines 1-1353 (fill-2): the rectilinear set-up that fill-3's graph walk runs on: `ExPolygonWithOffset` (the region turned, with its outer and inner offsets), `SegmentIntersection` with its exact rational comparison, `slice_region_by_vertical_lines`, `connect_segment_intersections_by_contours`, `pinch_contours_insert_phony_outer_intersections`, the helpers that measure and emit pieces of a perimeter, and the fillers of domain A's rectilinear family (`FillRectilinear`, `FillMonotonic`, `FillMonotonicLines`), whose `fill_surface` is fill-3's. |
| `stop` | The crate's stops, each with orca-index's `NotPorted` payload, and the texts of their conditions. |

Each module's header comment says what the C++ does that the translation keeps.

## What stops, and why

- The rectilinear fillers' `fill_surface` (FillRectilinear.cpp from line 1354) is fill-3's; until
  fill-3 writes it, it stops with the text "FillRectilinear.cpp's code that orca-o3-fill-3 ports".
  The fillers and the set-up exist now so that fill-3 adds only the walk.
- Both `connect_base_support` forms (FillBase.cpp 1834-2711, no piece's lines) stop with the family
  `infill-supportbase`: only the support base pattern calls them, and supports mark a job outside
  domain A.
- Inputs the C++ engine's release build has no answer for stop instead of reading through a null
  pointer or dividing by zero: `_infill_direction` on a surface 0 layers thick, a T-joint whose
  neighbour on its contour was never set, and `take_limited` given a line without points (fill-1);
  `connect_infill` given a line without points or a boundary contour without points, an
  intersection link of -1 followed in the rectilinear set-up, top solid infill on a filler without
  a print object config, and the gap fill's filter read from parameters without a region config
  (fill-2). The engine never makes these.

Left out, or written by another piece: `adapt_fill_octree` (only the adaptive and support cubic
fillers read it, both outside domain A); `Fill::new_from_type` and `Fill::use_bridge_flow`
(FillBase.cpp 40-104), fill-4's; `multiline_fill`, fill-3's; `mark_boundary_segments_overlapping_infill`
(only `connect_base_support` calls it) and `interpolate_contour_point` (nothing calls it); the
rectilinear fillers outside domain A (aligned rectilinear, grid, lateral lattice, triangles, stars,
cubic, quarter cubic, lateral honeycomb, support base, zig-zag, cross zag, locked zag) and
`FillMonotonicLine`, which no `Fill::new_from_type` case makes; the three `sample_grid_pattern` forms
(the lightning infill's); `validate_segment_intersection_connectivity` and the `#ifndef NDEBUG` and
`SLIC3R_DEBUG` checks that only asserts call, the `INFILL_DEBUG_OUTPUT` SVG export and the code
inside `#if 0`; the `reserve` calls, which change nothing that can be read.

## How it is proven

`tests/probes.rs` replays the records of `fill.sop1` (`parity/SOP1.md`, "Probe files", and
`parity/sop1/fill.md`), which `bun run engine parity probe --only fill` writes from the C++
engine's own build (`parity/oracle/probe/fill.cpp` and `fill/`). fill-1 replays FLDF (the defaults
of `FillParams`, of a new filler and of `Surface()`, and the anchors' statics), FLPA (`full_infill`
and `dont_connect`), FLAS (`_adjust_solid_spacing`), FLDR (`_infill_direction` with what it prints,
and `_layer_angle`) and FLSU (`fill_surface` and `fill_surface_arachne` through a filler of the
test's own that keeps what `_fill_surface_single` is given). fill-2 replays FLCN (the three
`connect_infill` forms and `chain_or_connect_infill` on lines clipped by the boundary, some chained,
some with an end drawn in), FLEB (`extended_object_bounding_box`), FLGP (`_create_gap_fill` on a
collection that already holds paths, with what the engine logs at error or fatal) and FLEX
(`fill_surface_extrusion` through a filler of the test's own that returns given lines or throws).
Each record is rebuilt whole from its inputs and the crate's results and compared with the engine's
byte for byte. The committed set holds 1 FLDF, 213 FLPA, 271 FLAS, 72 FLDR, 33 FLSU, 28 FLCN, 105
FLEB, 11 FLGP and 13 FLEX records; the full set 1, 413, 621, 325, 341, 445, 105, 214 and 264. FLRP
is fill-3's, FLCP, FLNT, FLNM and FLRA fill-4's; until they replay them, the test only checks that
no other kind is in the file. The `contour` module is reached only through `connect_infill`, which
the FLCN records prove. The rectilinear set-up is reached only through fill-3's `fill_surface`,
which its FLRP records will prove; until then its unit tests check the slicing, the exact
comparison, the links, the pinch and the runs on squares and diamonds by hand.

```sh
bun run rs:test -p orca-fill
SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes bun run rs:test:release -p orca-fill --test probes
```

## Dependencies

Every library crate of phase O2 and every crate of phase O3 before it in the build order:
`orca-arrange`, `orca-config`, `orca-crt`, `orca-eigen`, `orca-exact`, `orca-fmt`, `orca-geom`,
`orca-json`, `orca-mesh`, `orca-model`, `orca-placeholder`, `orca-qhull`, `orca-stl`, `orca-tbb`,
`orca-xml`, `orca-zip`, `orca-index`, `orca-paths`, `orca-voronoi`, `orca-slice`, `orca-layer`,
`orca-arachne`, `orca-print`, `orca-perimeter`, `orca-surface`. `orca-probe` for its tests. Nothing
from crates.io. fill-1 calls orca-geom (`Point`, `Polygon`, `ExPolygon`, `Polyline`, the bounding
boxes, `offset_ex` of an ExPolygon, `lerp`, `perp`, the Liang-Barsky clip and the ray and circle
intersection), orca-index (`EdgeGrid::Grid` and the `NotPorted` stop), orca-eigen (`Vec2d`),
orca-layer (`Surface`, `Flow`, `ExtrusionRole` and the extrusion entities), orca-config
(`InfillPattern` and the print, object and region configs), orca-stl (`min`, `max`,
`lower_bound_by`), orca-crt (`sqrt`, `floor` and the float-to-integer cast) and orca-fmt
(`stdio::stdout_write`, C's `stdout`, for `_infill_direction`'s `printf`; the probe test takes it
with `stdio::set_stdout_sink`). fill-2 adds orca-voronoi (the medial axis of the gaps, whose log
goes through orca-mesh's `log`, which the probe test takes with `log::set_sink`), orca-paths
(`chain_polylines`, `chain_points`), orca-index's `EdgeGrid` for the boundary graph, orca-layer's
`variable_width`, extrusion entities and `Flow::with_spacing`, orca-config's `ConfigBase::option`
for the flow rate calibration's flag, and orca-geom's Clipper offsets (`offset`, `shrink`,
`opening_ex`, `offset2_ex`, `diff_ex`, `union_ex`) and polygon clean-up.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.
