# orca-slice

Phase O3 of the Rust translation: libslic3r's mesh slicer (TriangleMeshSlicer), SlicingParameters,
adaptive slicing and the layer height series.

Set up empty by orca-o3-skeleton on 2026-11-13: a manifest that declares every dependency the crate
will need, so that no unit that fills it changes `Cargo.toml` or `Cargo.lock`, and a `src/lib.rs`
with the header and the crate's doc comment. Each piece below adds its modules and its tests and
keeps this README true.

## The pieces that fill it

| Piece | What it adds |
| --- | --- |
| `orca-o3-slice-1` | TriangleMeshSlicer's first half and SlicingParameters (done 2026-11-13) |
| `orca-o3-slice-2` | TriangleMeshSlicer's second half, adaptive slicing and the layer series (done 2026-11-14) |

The C++ they translate (under `src/libslic3r/` of OrcaSlicer v2.4.2 unless named):
TriangleMeshSlicer.hpp, TriangleMeshSlicer.cpp, Slicing.hpp, Slicing.cpp, SlicingAdaptive.hpp,
SlicingAdaptive.cpp.

## The modules

- `src/triangle_mesh_slicer.rs` (slice-1): `TriangleMeshSlicer.hpp` (`MeshSlicingParams` and
  `MeshSlicingParamsEx`) and `TriangleMeshSlicer.cpp` lines 1-1057, with `next_idx_modulo` of
  `libslic3r.h` (it belongs in orca-geom). `slice_facet` gives the intersection line of a facet
  with a plane and how it lies (a general crossing, an edge on the plane at the top or the bottom,
  a horizontal facet); `slice_make_lines` gives the lines of every face at every `z`, and
  `slice_make_lines_plane` those of the faces a filter keeps at one plane;
  `slice_facet_with_slabs` and `slice_slabs_make_lines` give the lines of the faces looking up or
  down, at each slice and between slices, which `slice_mesh_slabs` projects; `OpenPolyline` is the
  open loop the chaining of slice-2 works on. The C++ pushes each line under one of 64 mutexes;
  under the oracle's one thread that order is the order of the faces. The port gives each leaf of
  the parallel loop its own list and joins the lists by the leaf's first face, so the lines come in
  the order of the faces in any thread arena; the unit tests check it in an arena of 8 run by one
  thread (the oracle's) and in one of 4 threads.
- `src/slicing.rs` (slice-1): `Slicing.hpp` and `Slicing.cpp` lines 1-306.
  `SlicingParameters::create_from_config` (the first layer, the layer height limits over the
  support filaments and the object's nozzles, the zero-gap interfaces, the gaps rounded to whole
  layers, the raft), `equal_layering`, the nozzle limits of namespace `Slicing`, and
  `layer_height_profile_from_ranges`, with the header's `HeightProfileSmoothingParams` and
  `LayerHeightEditActionType` that slice-2's functions take.
- `src/slice_mesh.rs` (slice-2): `TriangleMeshSlicer.cpp` lines 1058-2159 and the declarations of
  `slice_mesh`, `slice_mesh_ex` (with its two inline overloads, `slice_mesh_ex_default` and
  `slice_mesh_ex_closing`) and `slice_mesh_slabs`. A layer's lines are chained by triangle
  connectivity (the lines sorted by `edge_a_id` and `a_id` with orca-stl's sort, the first unused
  of equals taken), then the open polylines are joined through their end ids in one direction and
  then in either, and through the nearest end within 2 mm (orca-paths'
  `ClosestPointInRadiusLookup`), each round seeded from the polylines sorted by length; a loop
  patched from pieces of either direction is turned counter-clockwise. The layers run in a
  `tbb::parallel_for` and each writes only its own polygons, so the result is the same in any
  arena. `slice_mesh_ex` adds `make_expolygons` (union by the mode's fill type, the closing and
  extra offsets, the largest contour, the simplification); `slice_mesh_slabs` classifies the faces
  by their projected cross product (its sign from the dynamic block's LU determinant) and chains
  each slab's lines by connectivity alone.
- `src/slicing_adaptive.rs` (slice-2): `SlicingAdaptive.hpp` and `SlicingAdaptive.cpp`. `prepare`
  keeps each face's z span and the cosine and sine of its normal to Z, sorted by span;
  `next_layer_height` gives the height the faces' slopes allow at a quality from 0 to 1, mixing
  `float` and `double` as the C++ does; `horizontal_facet_distance` the distance to the next flat
  face or the top.
- `src/slicing_layers.rs` (slice-2): `Slicing.cpp` lines 307-901, with `lerp` of `libslic3r.h`
  (it belongs in orca-geom). `layer_height_profile_adaptive`, `smooth_height_profile` (six rounds
  of a biased gaussian blur), `adjust_layer_height_profile` (the layer height editor's brush, its
  `size_t` indices wrapping as the C++'s do), `generate_object_layers` (with the Z shrinkage
  compensation and BBS's `adjust_layer_series_to_align_object_height`) and
  `check_object_layers_fixed`.

## What is left out, and what stops

slice-1 leaves out, as its module headers say:

- the declarations of `TriangleMeshSlicer.hpp` whose bodies are past line 1057 (`slice_mesh`,
  `slice_mesh_ex` and its inline overloads, `slice_mesh_slabs`): slice-2 writes them with their
  bodies. `project_mesh` and `cut_mesh` are not carried: no piece writes their bodies (2161 on), and
  the engine reaches `cut_mesh` only through the cut tool.
- `is_equal`, `is_equal_for_sort` and `slice_facet_for_cut_mesh` (`TriangleMeshSlicer.cpp` 44-56
  and 322-473): only `cut_mesh` calls them. `remove_tangent_edges` (977-1041) is `#if 0` in the C++.
- the declarations of `Slicing.hpp` whose bodies are past line 306 (slice-2), and the GUI's
  `generate_layer_height_texture` and the cereal `serialize` of a height range.

slice-2 leaves out `make_expolygons_simple` (`TriangleMeshSlicer.cpp` 1664-1736), which only
`triangulate_slice` and so `cut_mesh` call; the GUI's layer height texture
(`generate_layer_height_texture`, `Slicing.cpp` 903 on); and the debug output and asserts the
release build compiles out (`SLIC3R_DEBUG_SLICE_PROCESSING`, `ADAPTIVE_LAYER_HEIGHT_DEBUG` and the
rest).

These calls stop with orca-index's `NotPorted` where the C++ has no answer in its release build:

- `layer_height_profile_from_ranges` on a height range whose config holds no float
  `layer_height` (condition `range-without-layer-height`): the C++ dereferences the null option.
  The command line's ranges always hold one.
- `Slicing::min_layer_height_from_nozzle` and `max_layer_height_from_nozzle` on a config without
  the nozzle option as a non-empty list of floats (condition `nozzle-option-missing`): the C++
  dereferences a null option or reads an empty vector. Only Orca's GUI calls them.
- `layer_height_profile_adaptive` on a height range the layers reach whose config holds no float
  `layer_height` (the same condition, `range-without-layer-height`): `opt_float`'s `dynamic_cast`
  is null there.
- `SlicingAdaptive::prepare` on an object without an instance (`object-without-instance`): the C++
  reads `instances.front()` of an empty vector. Every object the engine slices has one.
- `adjust_layer_height_profile`, `smooth_height_profile` and `generate_object_layers` where the C++
  reads or writes the profile out of its range (`profile-index-out-of-range`): a band that starts
  at the bottom of a profile whose first layer is not fixed (the index wraps past 0), or a profile
  of an odd count. Only the GUI's layer height editor calls `adjust_layer_height_profile`.
- `chain_open_polylines_exact` in either direction on an open polyline end with neither a vertex
  nor an edge (`open-polyline-end-without-reference`): the C++ dereferences the end iterator. The
  chaining by connectivity never makes such an end.

One answer is the port's own: in `slice_mesh` (the polygons) a PositiveLargestContour layer whose
loops all have an area of 0 makes the C++ dereference a null pointer; the port keeps that layer's
first loop as it is. The engine never gets there (`slice_mesh_ex` slices that mode as Positive),
and no record holds such a layer (`parity/sop1/slice.md`).

## How it is proven

`tests/probes.rs` replays the records of `slice.sop1` (`parity/SOP1.md`, "Probe files", and
`parity/sop1/slice.md`), which `bun run engine parity probe --only slice` writes from the C++
engine's own build (`parity/oracle/probe/slice.cpp`), and compares every value by its bits.
slice-1's kinds: SLPR (`create_from_config` before and after one more setting, `equal_layering`
and the nozzle limits for every index from 0, nozzle 0 reading the first value as the C++'s
`size_t(-1)` does) and SLRG (`layer_height_profile_from_ranges`). slice-1's lines of
`TriangleMeshSlicer.cpp` have no kind of their own: the engine reaches them only through
`slice_mesh`, `slice_mesh_ex` and `slice_mesh_slabs`, so slice-2's SLMS, SLM1, SLMX and SLSB prove
them. slice-2's kinds, in `tests/probes/slice_2.rs`, run in the oracle's arena (8 slots, one
thread): SLMS, SLM1 and SLMX (`slice_mesh` at heights and at one plane, `slice_mesh_ex`, on the
table's meshes, flipped where the record says), SLSB (`slice_mesh_slabs` on the symmetric meshes),
SLAD (`SlicingAdaptive`'s faces and each recorded call), SLAP, SLSM, SLAJ and SLOL (the adaptive
profile, the smoothing, the brush, the layer series and the fixed test). Objects are built as the
probe builds them, and whether the instance is left-handed is checked against the record. The unit
tests check the lines' order, the chaining across a gap, the slicing modes, the result in a serial
and a parallel arena, and the stops. `every_kind_is_known` fails on a kind no piece replays.

The committed set runs by default (`bun run rs:test -p orca-slice`). The full set replays with
`SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes bun run rs:test:release -p orca-slice --test probes`.

## Dependencies

Every library crate of phase O2 and every crate of phase O3 before it in the build order:
`orca-arrange`, `orca-config`, `orca-crt`, `orca-eigen`, `orca-exact`, `orca-fmt`, `orca-geom`,
`orca-json`, `orca-mesh`, `orca-model`, `orca-placeholder`, `orca-qhull`, `orca-stl`, `orca-tbb`,
`orca-xml`, `orca-zip`, `orca-index`, `orca-paths`, `orca-voronoi`. `orca-probe` for its tests.
Nothing from crates.io.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.
