# orca-surface

Phase O3 of the Rust translation: libslic3r's MutablePolygon, region expansion, the surface types,
the vertical and horizontal shells, prepare_infill, the external surfaces and bridges over infill.

Set up empty by orca-o3-skeleton on 2026-11-13: a manifest that declares every dependency the crate
will need, so that no unit that fills it changes `Cargo.toml` or `Cargo.lock`, and a `src/lib.rs`
with the header and the crate's doc comment. Each piece below adds its modules and its tests and
keeps this README true.

## The pieces that fill it

| Piece | What it adds |
| --- | --- |
| `orca-o3-surfaces-1` | MutablePolygon and region expansion (2026-11-13) |
| `orca-o3-surfaces-2` | Surface types and the vertical and horizontal shells (2026-11-14) |
| `orca-o3-surfaces-3` | Prepare_infill, the external surfaces and bridges over infill |

The C++ they translate (under `src/libslic3r/` of OrcaSlicer v2.4.2 unless named):
MutablePolygon.hpp and .cpp, Algorithm/RegionExpansion.hpp and .cpp, PrintObject.cpp,
LayerRegion.cpp, utils.cpp.

## The modules

| Module | What it carries |
| --- | --- |
| `mutable_polygon` | `MutablePolygon.hpp` and `MutablePolygon.cpp` whole (surfaces-1): `MutablePolygon`, a loop of doubly linked points in one vector with a free list; `Iter`, the C++ iterator's index (the polygon is the argument of each step); `Range`, the range of unprocessed points; the three `remove_duplicates` of a `MutablePolygon` and the ExPolygons one; `smooth_outward` of a `MutablePolygon`, a `Polygon`, `Polygons` and `ExPolygons`, with the static `clip_narrow_corner`. |
| `region_expansion` | `Algorithm/RegionExpansion.hpp` and `.cpp` whole (surfaces-1): `RegionExpansionParameters::build`, `WaveSeed` and its two orders, `wave_seeds` (ClipperLib_Z with the Z fill visitor, `merge_splits`, the AABB tree over the boundaries' boxes), the wave propagation (`propagate_waves`, `_src`, `_steps`, `propagate_waves_ex`, `_ex_steps`), `expand_expolygons`, `merge_expansions_into_expolygons` and `expand_merge_expolygons`. The C++ overloads are functions named for their arguments. |
| `clipper_z_utils` | `ClipperZUtils.hpp` whole and the `ClipperLib_Z::Path` form of Polyline.hpp's template `polylines_merge` (surfaces-1). Both belong in orca-geom, beside `clipper_z` and `polyline::polylines_merge`; no piece of phase O3 carries them there, so they are here. |
| `detect_surfaces` | `PrintObject.cpp` 1520-1925 (surfaces-2): `PrintObject::detect_surfaces_type` as `detect_surfaces_type(print, object_idx)`: each region's slices split into top, bottom (bridging unless supports carry it) and internal, the thin-membrane overlaps made bottom save small cracks, the spiral vase layers, ORCA's second bridge layer (`enable_extra_bridge_layer`) and its interim retyping to bottom bridge, the fill surfaces clipped from the slices, and `typed_slices`. |
| `vertical_shells` | `PrintObject.cpp` 2007-2448 (surfaces-2): `PrintObject::discover_vertical_shells` as `discover_vertical_shells(print, object_idx)`: the per-layer cache of grown tops, bottoms and holes (over all regions, or per region), the shell projected from the layers within the shell thickness, regularized by square-joined opening and closing, the tiny drops filtered out, the result made internal solid. |
| `horizontal_shells` | `PrintObject.cpp` 3869-4288 (surfaces-2): `clip_fill_surfaces` (off in the engine: `infill_only_where_needed` is a `false` constant; the body is translated), `discover_horizontal_shells` (the `extra_solid_infills` layers, the tops and bottoms scattered through the shell layers, the narrow parts dropped or grown) and `combine_infill` (the runs of layers up to the nozzle or `infill_combination_max_layer_height`, their common infill printed once on top), each `(print, object_idx)`. |
| `layer_region` | `LayerRegion.cpp` 935-988 (surfaces-2): the trait `LayerRegionFill` on orca-layer's `LayerRegion` with `prepare_fill_surfaces(object, print)`, `infill_area_threshold(layer, object, print)` and `trim_surfaces(polygons)`; orca-layer leaves these members to this crate. |
| `utils` | `utils.cpp` 1749-1830 (surfaces-2): `check_layer_id_pattern(pattern, layer_id)`, the `extra_solid_infills` pattern (`N`, `N#K` and comma lists) read with `orca_fmt::parse::stoi` (MSVC's `strtol`), the pattern as bytes. It belongs in a crate for `utils.cpp` that no piece carries. |
| `layers_out` | No C++ (surfaces-2): `LayersOut`, which takes an object's layers out of it for a step so that the step changes them while it reads the object and the print (the flows read neither's layers), and puts them back on drop, a panic included; the parallel-loop helper `map_range`; and helpers that belong elsewhere: `union_safety_offset_ex` of ExPolygons (orca-geom), `is_auto` (orca-config), the `nozzle_diameter.get_at(filament - 1)` read (orca-layer's is private) and the supports stop. |

Each module's header comment says what the C++ does that the translation keeps.

## What stops, and why

Nothing of surfaces-1 stops with `NotPorted`: no function of these files reads a setting, so none
reaches a family outside domain A. Domain A reaches region expansion through LayerRegion's bridge
and surface expansion (surfaces-3) and never reaches MutablePolygon (`smooth_outward` is called by
`PrintObject::clip_fill_surfaces`, which returns at once as `infill_only_where_needed` is false,
and by the supports; the ExPolygons `remove_duplicates` by the multi-material segmentation); the
crate carries all of it all the same.

Where the C++ reads outside a vector (undefined behaviour the engine never meets), the translation
panics with a message naming the call: a `MutablePolygon` iterator that is not valid, an empty seed
or clipped path, an intersection index or a source id past its vector, and a source without a
contour in `merge_expansions_into_expolygons`.

Left out: `MutablePolygon::capacity()`, which no engine code calls (Rust's vector grows by its own
rule), and the `reserve` and `reserve_more_power_of_2` calls, which change nothing that can be read.

surfaces-2's steps stop with `NotPorted` where domain A never goes: `detect_surfaces_type` on an
object with supports (`has_support()`, read in each leaf of its classifying loop), naming the
supports or tree-supports family by `support_type`; `combine_infill` on a first layer whose id is
not 0 (a raft below the object) that is as thick as the combined height, where the C++ writes
`combine[-1]`. Where the C++ has no answer it stops too: `combine_infill` on an object with no
layers (`combine[size() - 1]`) and an empty `nozzle_diameter` (`get_at` reads its front). The rest
is translated whole, `clip_fill_surfaces`'s body included. Left out: `log_memory_info()` from the
two info messages (`"Detecting solid surfaces..."`, `"Discovering vertical shells..."`), as the
process's memory sizes differ from run to run (HAZARDS.md); the SVG exports of
`SLIC3R_DEBUG_SLICE_PROCESSING`, the `#if 0` blocks, the `false` constant
`one_more_layer_below_top_bottom_surfaces`'s branches, `PROFILE_FUNC()` and the debug build's
assert in `trim_surfaces`, none of which the engine builds.

The C++ members of `PrintObject` become free functions `(print: &mut Print, object_idx)` that take
the object's layers out (`layers_out::LayersOut`) and return a flow's `FlowError` where the C++
throws it. Their `tbb::parallel_for` loops run through `orca_tbb::parallel_for` on the same
`blocked_range` and partitioner (`layers_out::map_range`): each iteration of these loops reads the
layers and changes only its own layer, which no other iteration reads, so each change is computed
from the unchanged layers and applied in index order afterwards, and the result is the C++'s
whatever order the leaves run in (each module's header says why that holds of its loops).
orca-o3-surfaces-3's `PrintObject::prepare_infill` calls them in the C++'s order:
`detect_surfaces_type`, `LayerRegionFill::prepare_fill_surfaces` for each region of each layer
(through a `LayersOut`), `discover_vertical_shells`, `discover_horizontal_shells`, then after the
external surfaces `clip_fill_surfaces`, and after bridge over infill `combine_infill`.

## How it is proven

`tests/probes.rs` replays the records of `surfaces.sop1` (`parity/SOP1.md`, "Probe files", and
`parity/sop1/surfaces.md`), which `bun run engine parity probe --only surfaces` writes from the C++
engine's own build (`parity/oracle/probe/surfaces.cpp` and `surfaces/`). surfaces-1 replays SFMP (a
script of `MutablePolygon` steps and the state after each), SFRD (`remove_duplicates`), SFSO
(`smooth_outward`), SFRP (`RegionExpansionParameters::build`), SFRX (one source and boundary
through every public function of `RegionExpansion.hpp`, the sorted seeds as the permutation
`std::sort` makes) and SFME (`merge_expansions_into_expolygons`); points are compared exactly and
every `float` and `double` by its bits. surfaces-2 replays SFLP (`check_layer_id_pattern` of 67
hand-made and 300 seeded patterns, read as bytes, at the layer indices -2 to 60 and the ends of
`int`). SFBR and SFBF are surfaces-3's; until it replays them, the test only checks that no other
kind is in the file.

surfaces-2's steps read a print object and its layers, which only `PrintObject` makes, so no probe
records them: orca-o3-dumps' step dumps prove `detect_surfaces_type`, `discover_vertical_shells`,
`clip_fill_surfaces`, `discover_horizontal_shells`, `combine_infill` and the `LayerRegion` members
against the C++ engine (`parity/sop1/surfaces.md`). `tests/steps.rs` runs each step on stacks of
squares built in the test, whose surface types follow from the C++ by hand: bottom, internal and
top; an overhang's bottom bridge; spiral vase mode; the supports stop with the layers put back;
the horizontal and vertical shells; a combined run of infill; `prepare_fill_surfaces`,
`infill_area_threshold` and `trim_surfaces`.

```sh
bun run rs:test -p orca-surface
SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes bun run rs:test:release -p orca-surface --test probes
```

## Dependencies

Every library crate of phase O2 and every crate of phase O3 before it in the build order:
`orca-arrange`, `orca-config`, `orca-crt`, `orca-eigen`, `orca-exact`, `orca-fmt`, `orca-geom`,
`orca-json`, `orca-mesh`, `orca-model`, `orca-placeholder`, `orca-qhull`, `orca-stl`, `orca-tbb`,
`orca-xml`, `orca-zip`, `orca-index`, `orca-paths`, `orca-voronoi`, `orca-slice`, `orca-layer`,
`orca-arachne`, `orca-print`, `orca-perimeter`. `orca-probe` for its tests. Nothing from crates.io.
surfaces-1 calls orca-geom (`Point`, `Polygon`, `ExPolygon`, `BoundingBox`, Clipper's `Clipper`,
`ClipperOffset` and `Orientation`, ClipperLib_Z, the ClipperUtils unions and the bounding box clip),
orca-index (the AABB tree and `BoundingBoxWrapper`), orca-eigen (`Vec2d`'s dot products and squared
norms), orca-stl (`sort`, `sort_by`, `unique`, `lower_bound_by`, `min`, `max`) and orca-crt (`cos`,
`acos`, `sqrt`, `ceilf`, `floor` and the float-to-integer casts). surfaces-2 calls orca-layer
(`Print`, `PrintObject`, `Layer`, `LayerRegion` and its flows, `Surface`, `SurfaceCollection`),
orca-config (the print, object and region settings), orca-geom (the ClipperUtils offsets,
openings, closings, unions, differences and intersections), orca-tbb (`parallel_for` and
`parallel_for_each_mut`), orca-fmt (`stoi`), orca-stl (`min`, `max`), orca-crt (`fabs`),
orca-mesh (the log) and orca-index (`not_ported`).

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.
