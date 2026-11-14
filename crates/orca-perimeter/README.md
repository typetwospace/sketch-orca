# orca-perimeter

Phase O3 of the Rust translation: libslic3r's PerimeterGenerator, classic and Arachne, with
make_perimeters, BridgeDetector and the 2D principal components.

Set up empty by orca-o3-skeleton on 2026-11-13: a manifest that declares every dependency the crate
will need, so that no unit that fills it changes `Cargo.toml` or `Cargo.lock`, and a `src/lib.rs`
with the header and the crate's doc comment. Each piece below adds its modules and its tests and
keeps this README true.

## The pieces that fill it

| Piece | What it adds |
| --- | --- |
| `orca-o3-walls-1` | PerimeterGenerator's helpers, BridgeDetector and principal components (2026-11-14) |
| `orca-o3-walls-2` | The classic perimeters |
| `orca-o3-walls-3` | The Arachne perimeters and make_perimeters |

The C++ they translate (under `src/libslic3r/` of OrcaSlicer v2.4.2 unless named):
PerimeterGenerator.hpp and .cpp, BridgeDetector.hpp and .cpp, PrincipalComponents2D.hpp and .cpp,
Arachne/utils/ExtrusionLine.hpp and .cpp, Layer.cpp, LayerRegion.cpp, PrintObject.cpp.

## The modules

Each module's header comment says how it works and which C++ lines it carries.

| Module | What it carries |
| --- | --- |
| `principal_components` | PrincipalComponents2D.hpp and .cpp: the moments of a triangle and the principal components of polygons, in the C++'s mix of `float` and `double` |
| `bridge_detector` | BridgeDetector.hpp and .cpp: `BridgeDetector` (anchors, `detect_angle`, `coverage`, `unsupported_edges`) and the newer `detect_bridging_direction`, both overloads; Polygon.hpp's `polygons_rotate` |
| `extrusion_paths` | Arachne/utils/ExtrusionLine.hpp 287-292 and .cpp 289-303: `extrusion_paths_append` of ClipperLib_Z paths and of an Arachne line; libslic3r.h's `append` of two path lists |
| `extra_perimeters` | PerimeterGenerator.cpp 673-1085: the extra perimeters on overhangs (`generate_extra_perimeters_over_overhangs`, `sort_extra_perimeters`, `paths_touch`, `reconnect_polylines`); Polygon.hpp's `to_lines` of polygons |
| `perimeter_generator` | PerimeterGenerator.hpp 1-11 and 48-173, PerimeterGenerator.cpp 1-672 and 1086-1143 (and 2537-2547, `is_internal_contour`): the generator's state, `detect_steep_overhang`, `traverse_loops`, `clip_extrusion`, `traverse_extrusions`, `split_top_surfaces`, `apply_extra_perimeters`, `reorient_perimeters` |
| `fuzzy_skin` | PerimeterGenerator.hpp 12-47 (`FuzzySkinConfig`) and Feature/FuzzySkin/FuzzySkin.cpp's `should_fuzzify` and the parts of the two `apply_fuzzy_skin` that leave a wall alone |
| `stop` | The crate's stops (orca-index's `NotPorted`) and `PerimeterError` (a flow error or a Voronoi error) |

`process_classic`, `process_no_bridge` and `generate_lower_polygons_series` are walls-2's;
`process_arachne` and `add_infill_contour_for_arachne` are walls-3's.

The C++ generator holds pointers to its inputs and outputs. Here the inputs are references (an
`Option` where the pointer may be null) and the four outputs are owned: the caller moves them in
through `PerimeterGenerator::new` and takes them back. A compatible region is a layer region with
its print region's configuration. `PerimeterGeneratorArachneExtrusion` names its line by index into
the list `traverse_extrusions` is given.

## What stops with NotPorted, and why

Domain A never reaches these, so the engine never stops in them. Each stop names its function and
its condition.

- `fuzzy-skin` (`apply_fuzzy_skin`, both forms): a region of the generator would roughen the wall.
  Domain A has no fuzzy skin, so the walls stay as they are; the noise itself is not translated.
- `raft` is the family name the processes (walls-2, walls-3) stop with for `raft_layers` above 0.
- Inputs on which the C++ has undefined behaviour: a spacing not above 0 (`detect_angle` divides by
  it, `get_trapezoids3_half` loops forever), an index of the first unanchored path past the end
  (`sort_extra_perimeters`), an empty polygon (`to_lines`), an empty lower polygon series
  (`traverse_loops`), a loop whose overhang paths are all empty (`traverse_loops` casts a thin wall
  to a loop), no lower slices where overhangs are tested (`detect_steep_overhang`), no upper slices
  in `split_top_surfaces`, and no islands' collection in `apply_extra_perimeters`.

## Helpers translated here that belong elsewhere

- `polygons_rotate` (bridge_detector) and `to_lines_polygons` (extra_perimeters): orca-geom.
- `append_paths` (extrusion_paths): orca-layer's `cpp_vec`.
- `FuzzySkinConfig`, `should_fuzzify` and the `apply_fuzzy_skin` minimum (fuzzy_skin): a fuzzy-skin
  crate that does not exist yet.

## How it is proven

`tests/probes.rs` replays the records of `walls.sop1` (`parity/sop1/walls.md`), which `bun run
engine parity probe --only walls` writes from the C++ engine's own build
(`parity/oracle/probe/walls.cpp` and `walls/`). It reads the committed set in `parity/probes/`, or
the full set when `SKETCH_ORCA_PROBES_DEEP` names its folder, rebuilds each record from the Rust
and compares the bytes. walls-1's kinds: WLMT, WLPC, WLDB, WLBD, WLEA, WLRC, WLPT, WLSX and WLXP.
All matched on 2026-11-14 (261 records in the committed set, debug; 1321 in the full set,
release). WLTP, WLCP, WLCL and WLAR belong to walls-2 and walls-3; the test counts them and leaves
them. `perimeter_generator`'s functions are reached only through the two processes, so walls-2's
WLCL and walls-3's WLAR prove them.

## Dependencies

Every library crate of phase O2 and every crate of phase O3 before it in the build order:
`orca-arrange`, `orca-config`, `orca-crt`, `orca-eigen`, `orca-exact`, `orca-fmt`, `orca-geom`,
`orca-json`, `orca-mesh`, `orca-model`, `orca-placeholder`, `orca-qhull`, `orca-stl`, `orca-tbb`,
`orca-xml`, `orca-zip`, `orca-index`, `orca-paths`, `orca-voronoi`, `orca-slice`, `orca-layer`,
`orca-arachne`, `orca-print`. `orca-probe` for its tests. Nothing from crates.io.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.
