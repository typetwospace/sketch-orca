# orca-index

Phase O3 of the Rust translation: libslic3r's spatial indexes: the indirect AABB trees over
triangles and lines, the indirect KD tree, the mutable priority queue and EdgeGrid.

Set up empty by orca-o3-skeleton on 2026-11-13: a manifest that declares every dependency the crate
will need, so that no unit that fills it changes `Cargo.toml` or `Cargo.lock`, and a `src/lib.rs`
with the header and the crate's doc comment. Each piece below adds its modules and its tests and
keeps this README true.

## The pieces that fill it

| Piece | What it adds |
| --- | --- |
| `orca-o3-index-1` | The AABB trees, the KD tree and the mutable priority queue (2026-11-13) |
| `orca-o3-index-2` | EdgeGrid (2026-11-13) |

The C++ they translate (under `src/libslic3r/` of OrcaSlicer v2.4.2 unless named):
AABBTreeIndirect.hpp, AABBTreeLines.hpp, KDTreeIndirect.hpp, MutablePriorityQueue.hpp, EdgeGrid.hpp,
EdgeGrid.cpp.

## The modules

index-1's are header-only templates in the C++; here they are generic over the same scalars, so a
tree over a `float` mesh stays `f32`, and they keep the C++'s traversal order and tie rules.

| Module | What it carries |
| --- | --- |
| `aligned_box` | Eigen 5.0.1's `AlignedBox` (its empty box, `extend`, `contains`, `intersects`, `diagonal`, `cast` and `squaredExteriorDistance` of a point of another scalar), for `f32`, `f64` and `i64` boxes. It belongs in orca-eigen; no piece carries it there. |
| `aabb_tree` | `AABBTreeIndirect.hpp` whole: `Tree`, its build (the C++'s quickselect at the median along the longest side), `BoundingBoxWrapper`, the ray queries, the closest-point and radius queries over an indexed triangle set, `get_candidate_idxs`, `traverse` with `intersecting` and `within`. It also holds `next_highest_power_of_2` (`Utils.hpp`, which no crate carries yet) and `Hit` (libigl's `igl::Hit<float>`). |
| `aabb_lines` | `AABBTreeLines.hpp` whole: the tree over lines, the distance, radius, inside-or-outside and intersection queries and `LinesDistancer`, over `Line` (`coord_t`) and `Linef` (`double`). `Linef` and its two `line_alg` functions belong in orca-geom beside `Line`; no piece carries them there. |
| `kd_tree` | `KDTreeIndirect.hpp` whole: `KdTreeIndirect`, `visit` and `find_closest_points`, `find_closest_point` and both `find_nearby_points`. |
| `priority_queue` | `MutablePriorityQueue.hpp` whole: `MutablePriorityQueue`, `SkipHeapAddressing` and `MutableSkipHeapPriorityQueue`, with the index reset of the engine's release build (`NDEBUG`: only when `ResetIndexWhenRemoved` is true). |
| `edge_grid` | `EdgeGrid.hpp` and `EdgeGrid.cpp` (index-2): `Contour`, a view of a run of the caller's points, and `Grid`, which borrows what it is created from: its seven `create` overloads, `visit_cells_intersecting_line` and `visit_cells_intersecting_box`, `closest_point_signed_distance`, `signed_distance_edges`, `signed_distance`, `calculate_sdf`, `signed_distance_bilinear`, `contours_simplified`, `intersecting_edges`, `has_intersecting_edges` and the cell accessors; and the free `intersecting_edges(Polygons)`. A contour in an edge pair is its index in `contours()`, where the C++ keeps a pointer. It also holds `sort_remove_duplicates` (`libslic3r.h`), which belongs in orca-geom; no piece carries it there. |
| `stop` | `NotPorted`, the stop on the inputs the C++ has no answer for. |

Each module's header comment says what the C++ does that the translation keeps.

## What stops with NotPorted, and why

Where the engine's release build reads outside a vector the translation stops with a
`NotPorted` panic payload, which names the functions and the condition as
`parity/sop1/index.md`'s IXNP rows give them: the engine never runs on these inputs, and the C++
answer is undefined behaviour.

- `MutablePriorityQueue::top` and `pop` of an empty queue (they read the heap's first slot);
- `MutableSkipHeapPriorityQueue::top` and `pop` of an empty queue (they read slot 1);
- `MutablePriorityQueue::remove`, `update` and `operator[]` at an index of `size()` or more;
- `MutableSkipHeapPriorityQueue::remove`, `update` and `operator[]` at a padding slot, or at or
  past the heap's end.

Nothing of index-1 is outside domain A: the trees, the KD tree and the queues answer every other
input.

EdgeGrid (index-2) stops on index-2's eight IXNP rows:

- `EdgeGrid::Grid::create` with a resolution of 0 (`create_from_m_contours` divides by it) or below
  0 (rows and columns come out negative and the cell vector's size wraps), or whose rows times
  columns overflow `size_t` or cannot be allocated (past the vector's `max_size()`, or a failed
  allocation);
- `create`, `closest_point_signed_distance`, `signed_distance_edges`, `signed_distance` and
  `calculate_sdf` where a product of a coordinate difference and the resolution, or of two
  coordinate differences (or their sum), passes 2^63; a caller's `visit_cells_intersecting_line`
  whose products overflow stops with the same row;
- `visit_cells_intersecting_line` with an end point outside the grid's cells (below the box's
  minimum, or at or past the minimum plus the columns or rows times the resolution); `create`'s
  first raster pass stops the same way, which only a box that Eigen's compare past 2^31 made too
  small can reach;
- `signed_distance_bilinear` before `calculate_sdf` (it reads the empty field);
- the queries of a grid that was never created, wherever they read the resolution
  (`resolution()`, both cell walks, the three distances, `signed_distance_bilinear` and
  `calculate_sdf`);
- the free `intersecting_edges` when the mean edge length is below 1, so the resolution (the
  `double` turned into `coord_t`) is 0.

EdgeGrid is in domain A whole (elephant foot compensation reads it); the free
`intersecting_edges` serves the tree supports, outside it, and is translated all the same, as the
probe records it. A grid whose box was given with its minimum above its maximum and so has no
columns, or one created a second time with a larger grid after `calculate_sdf` (`create` keeps the
old field, as the C++ does), panics on a vector index where the C++ reads past the vector.

## Left out

- `AABBTreeIndirect::detail::intersect_triangle_epsilon`: its body calls `cwiseMax()` with no
  argument, so it does not compile once instantiated, and nothing in the engine calls it.
- The `detail::intersect_triangle` overloads whose origin is not `double`: they do not compile
  either (their `const V edge1` is a cast expression); every overload ends in the `double` one,
  which `aabb_tree::intersect_triangle` is.
- `EdgeGrid::Grid::intersect` (every overload), `Grid::inside`, `Grid::line_cell_intersect` and
  `div_floor`: they sit inside `#if 0`, so they are not compiled.
- `PropagateDanielssonSingleVStep3`: its two calls in `calculate_sdf` are commented out.
- `EdgeGrid::save_png`, `Slic3r::export_intersections_to_svg` and the `EDGE_GRID_DEBUG_OUTPUT`
  blocks (debug output), and the `#ifndef NDEBUG` checks, which the release build leaves out.

## How it is proven

`tests/probes.rs` replays the records of `index.sop1` (`parity/SOP1.md`, "Probe files", and
`parity/sop1/index.md`), which `bun run engine parity probe --only index` writes from the C++
engine's own build (`parity/oracle/probe/index.cpp` and `index/`). index-1 replays IXBX (the box
tree of RegionExpansion), IXTM (the triangle trees over float and double meshes), IXLT (the line
trees over `Line` and `Linef`), IXKD (the KD trees in 2 and 3 dimensions), IXPQ (the six queues
after every step) and its own IXNP rows (each call stops with the same texts); every value is
compared by its bits. index-2 replays IXEG (a grid from each `create` overload, with and without a
box set first: its box, size, contours and where each points into the input, cells, cell data,
both cell walks, the three distances, `contours_simplified`, the edge pairs, then the field and
what reads it), IXEI (the free `intersecting_edges`) and its eight IXNP rows. No other kind may be
in the file. Every record of the committed set and of the full set matched on 2026-11-13 (index-2's:
17 IXEG and 22 IXEI committed, 264 and 207 in the full set, and the 8 rows in both).

```sh
bun run rs:test -p orca-index
SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes bun run rs:test:release -p orca-index --test probes
```

## Dependencies

Every library crate of phase O2: `orca-arrange`, `orca-config`, `orca-crt`, `orca-eigen`,
`orca-exact`, `orca-fmt`, `orca-geom`, `orca-json`, `orca-mesh`, `orca-model`, `orca-placeholder`,
`orca-qhull`, `orca-stl`, `orca-tbb`, `orca-xml`, `orca-zip`. `orca-probe` for its tests. Nothing
from crates.io. index-1 calls orca-eigen (vectors, casts, `max_coeff_index`), orca-geom (`Point`,
`Line`, `BoundingBox`), orca-stl (`sort_by`, `lower_bound_by`, `min`, `max`) and orca-crt
(`fabs`, and `f64_to_i64` for the one float-to-integer cast). index-2 calls orca-geom (`Point`,
`Line`, `BoundingBox`, `Polygon`, `Polyline`, `ExPolygon`, `geometry::segments_intersect`),
orca-eigen (`norm`), orca-stl (`sort`, `unique_vec`, `min`, `max`) and orca-crt (`sqrt`,
`sqrtf`, `fabs`, `fabsf`, `floor`, `f64_to_i64`).

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only; the
modules that carry Eigen and libigl code (`aligned_box`, the `Hit` of `aabb_tree`) keep those
portions under MPL-2.0, as their headers say.
