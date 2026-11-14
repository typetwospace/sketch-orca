# index.sop1

The record kinds of the index probe (`parity/oracle/probe/index.cpp` and the sources of
`parity/oracle/probe/index/`, phase O3): the AABB trees, the KD tree, the mutable priority queue and
EdgeGrid, recorded by orca-o3-probe-index (2026-11-13) from the oracle's libslic3r. The values are
those of `parity/SOP1.md` ("The container", "Values" and "Probe files"). Every kind starts with `IX`,
and no other page defines it (`scripts/lib/parity/probe.ts`, `probeKindProblems`, which
`test/parity-probe.test.ts` runs). `orca-index` replays the records: index-1 ports
`AABBTreeIndirect.hpp`, `AABBTreeLines.hpp`, `KDTreeIndirect.hpp` and `MutablePriorityQueue.hpp`,
index-2 ports `EdgeGrid.hpp` and `EdgeGrid.cpp`. Each section names the piece that replays it, and
the crate's probe test rebuilds every record of its pieces from the Rust and compares it whole.

A record holds its inputs, then what the engine computed from them, so the replay needs no
generator. Inputs are shapes the probe builds (seeded with the splitmix64 of
`parity/oracle/probe/probe.hpp`) and the OBJ meshes of OrcaSlicer's tests
(`<exe>/../src/tests/data`, read with the engine's `load_obj`), never the corpus. Nothing here runs
on threads. Every record goes into the full set (`<work>/parity/probes/index.sop1`). The committed
set (`parity/probes/index.sop1`) takes each kind's pinned cases whatever their size, then its
hand-made cases of at most a byte limit, then the first seeded cases of at most that limit, up to a
count (`parity/oracle/probe/index/index.hpp`, `record_cases`, called once for the pinned cases with
no limit), so it is a true subset in the full set's order. The pinned cases are the sort cases
(below): IXTM's `stack` and `book` in float and IXLT's `zigzag` over `Line`, which pass their kind's
limit.

| kind | pinned | limit (bytes) | seeded cases committed | seeded cases in the full set |
| ---- | ------ | ------------- | ---------------------- | ---------------------------- |
| IXBX | 0 | 2048 | 24 | 400 |
| IXTM | 2 | 6144 | 8 | 120 (60 soups, float and double) |
| IXLT | 1 | 4096 | 12 | 600 (400 sets, half also as doubles) |
| IXKD | 0 | 3072 | 14 | 400 |
| IXPQ | 0 | 3072 | 18 | 300 |
| IXEG | 0 | 16384 | 2 | 240 |
| IXEI | 0 | 1536 | 16 | 200 |
| IXNP | 0 | none | all | none |

## index.sop1's values

Besides the values of `parity/SOP1.md` (`u8` to `u64`, `i32`, `i64`, `f32` and `f64` by their bits,
little-endian; `bool` a `u8` 0 or 1; `str` a `u32` byte count and UTF-8):

- a point: `i64` x, `i64` y; points: a `u64` count, then each point;
- indices: a `u64` count, then each `u64`. npos is 2^64 − 1 (`size_t(-1)`);
- a scalar: the record's coordinate type, `i64` for `coord_t`, `f32` for `float`, `f64` for
  `double`;
- nodes (an `AABBTreeIndirect::Tree`): a `u64` count, then each node's `u64` idx (npos for an unused
  node, 2^64 − 2 for an inner node, else the source item's index), then its box's minimum
  coordinates and its maximum coordinates as scalars. An unused node's box is Eigen's empty box:
  the minimum is the scalar's largest value and the maximum its lowest;
- a hit (`igl::Hit<float>`): `i32` id, `i32` gid, `f32` u, `f32` v, `f32` t.

## The pieces and their edges

index-1 replays IXBX, IXTM, IXLT, IXKD and IXPQ; index-2 replays IXEG and IXEI; each replays its
own IXNP rows. What the port must copy exactly, because the records depend on it:

- The AABB trees and the KD tree split at the median with their own quickselect
  (`partition_input`), so equal keys end where that function leaves them.
- `BoundingBoxWrapper` grows each box by `SCALED_EPSILON` on every side (IXBX holds the amount),
  and its centroid is `min + max / 2` in integers (truncated toward zero), not the box's middle.
- `build_aabb_tree_over_indexed_lines` takes `(a + b) * 0.5` as the centroid. For `Line` that is
  `Point`'s `operator*`: the sum as a `double` times 0.5, truncated toward zero to `coord_t`.
- `find_closest_points` inserts a point at the same distance as one already kept before it.
- `intersect_ray_all_hits` and the sorted `intersections_with_line` order by MSVC's `std::sort`
  (insertion sort up to 32 items, introsort above that). The port sorts with `orca_stl::sort_by`
  and the same comparator, so ties fall the same way at any count. IXTM's `stack` and `book` and
  IXLT's `zigzag` reach the introsort with ties (34 hits or intersections, in tied pairs or all at
  one t).
- The engine's release build defines `NDEBUG`: no assert runs, and `MutablePriorityQueue` resets
  the index of a removed item only when its `ResetIndexWhenRemoved` is true.
- EdgeGrid merges the contours' points into the box it was given (an undefined box is replaced by
  the first point), then widens it by 16 on each side; columns and rows are the box's sides divided
  by the resolution, rounded up.
- `contours_simplified` chains its lines through an `std::unordered_multimap`. Its result does not
  depend on the order that map returns equal keys in: at a corner the line turning left (cross
  product above 0) wins whichever comes first, and the loop closes when it meets its first line.

### IXBX

`AABBTreeIndirect::Tree<2, coord_t>` built with `build_modify_input` over `BoundingBoxWrapper`
boxes, as `Algorithm/RegionExpansion.cpp` builds it (index-1). `f64` `SCALING_FACTOR`; `i64` the
amount `BoundingBoxWrapper` adds to each side; a `u64` count of boxes, each point min and point max
(as given, before the wrapper grows them); the nodes. Then a `u64` count of queries, each `u8` kind,
point a, point b (0, 0 when unused), `u64` stop, then indices: the leaves' idx in the order found.
Kind 0 is `traverse` with RegionExpansion's predicate (the node's box contains a), kind 1
`traverse` with `intersecting(box(a, b))`, kind 2 `traverse` with `within(box(a, b))`, kind 3
`get_candidate_idxs(tree, a)`. The `traverse` callback keeps each leaf and stops the walk once it
holds stop leaves (0: never). Some boxes of kinds 1 and 2 are inverted (a above b).

### IXTM

`build_aabb_tree_over_indexed_triangle_set` and its queries (index-1): `Tree3f` over float
vertices as `AABBMesh.cpp` and `TriangleSelector.cpp` build it, and the same mesh in doubles.
`u8` 0 for float, 1 for double; `str` the mesh's label (`orca:<name>.obj` for a test mesh); a
`u64` count of vertices, each three scalars; a `u64` count of faces, each three `i32`; scalar
the epsilon the tree was built with; the nodes. (`detail::intersect_triangle_epsilon` is not
recorded: its body calls `cwiseMax()` with no argument, so it does not compile once instantiated,
and nothing in the engine calls it; a port leaves it out.)

Then a `u64` count of rays, each `f64` origin x, y, z, `f64` direction x, y, z, `f64` eps;
`bool` what `intersect_ray_first_hit` returned and, when true, its hit; `bool` what
`intersect_ray_all_hits` returned, then a `u64` count of its hits and each hit. The rays include a
zero direction.

Then a `u64` count of points, each `f64` x, y, z, `f64` r2; `f64` what
`squared_distance_to_indexed_triangle_set` returned, `u64` its face index (npos before the call),
`f64` x, y, z its closest point (0 before the call); `bool` `is_any_triangle_in_radius(r2)`;
indices `all_triangles_in_radius(r2)`; indices `get_candidate_idxs` at the point cast to the
scalar.

Then a `u64` count of boxes, each scalar min x, y, z and max x, y, z, `u64` stop, indices
`traverse` with `intersecting(box)`, indices `traverse` with `within(box)`, stopping as IXBX does.

### IXLT

`AABBTreeLines` (index-1): `build_aabb_tree_over_indexed_lines` and `LinesDistancer` over `Line`
(as `PerimeterGenerator.cpp`, `PrintObject.cpp`, `Fill.cpp` and `LineSplit.cpp` use it) or over
`Linef`. `u8` 0 for `Line` (scalar `i64`), 1 for `Linef` (scalar `f64`); a `u64` count of lines,
each scalar a x, a y, b x, b y; the nodes.

Then a `u64` count of points, each scalar x, y, `f64` max_sqr (infinity in some), `f64` radius;
`distance_from_lines_extra<false>` as `f64` distance, `u64` line, `f64` x, y of the nearest point;
`distance_from_lines_extra<true>` the same way; `i32` `outside()`;
`squared_distance_to_indexed_lines(lines, tree, point as Vec2d, line, hit, max_sqr)` as `f64` its
result (−1 with no line), `u64` the line (npos before the call), `f64` x, y of the hit (0 before
the call); indices `all_lines_in_radius(point, radius)`.

Then a `u64` count of query lines, each scalar a x, a y, b x, b y, then `intersections_with_line
<false>` and `intersections_with_line<true>`, each a `u64` count of hits, each scalar x, y and
`u64` the line.

### IXKD

`KDTreeIndirect<D, double>` (index-1), as `ShortestPath.cpp` builds it over the ends of paths
(D 2) and in 3D. `u8` D; a `u64` count of points, each D `f64`; `bool` built from a list of
indices, and when true the indices (else it was built from the count). Then the order `visit()`
reaches the nodes in when the visitor always goes on both ways: a `u64` count, each `u64` point
index and `u64` dimension.

Then a `u64` count of queries, each D `f64` the point, `u64` mod, `u64` rem, `f64` radius, D `f64`
lo, D `f64` hi; `u64` `find_closest_point`; three `u64` `find_closest_points<3>` (npos where there
are fewer); indices `find_nearby_points(point, radius)`; indices `find_nearby_points(lo, hi,
filter)`. The filter keeps point i when mod is 0 or i % mod ≠ rem. With mod 0 the first three use
the overloads without a filter; with mod 1 the filter drops every point.

### IXPQ

`MutablePriorityQueue.hpp` (index-1). The queue holds pointers to items, each with an `f64` key and
the `u64` index the queue's setter writes into it; less compares the keys. `u8` the queue: 0
`make_mutable_priority_queue<Item *, false>`, 1 the same with `true` (as `ShortestPath.cpp` has
them), 2 `make_miniheap_mutable_priority_queue<Item *, 32, false>` (as `QuadricEdgeCollapse.cpp`
has it), 3 the same with `true`, 4 and 5 with blocks of 4. A `u64` count of items and each item's
`f64` first key (every item's index starts at npos).

Then a `u64` count of steps, each `u8` code, `u64` a, `f64` key, then the queue after it. Code 0
sets item a's key and pushes it; 1 pops; 2 `remove(a)`; 3 sets the key of the item at heap address
a and calls `update(a)`; 4 `clear()`. A heap address is a slot of the heap; for the skip heaps the
slots at multiples of the block size are padding and never addressed. After each step: `u64`
`size()`, `u64` n the heap's slots up to the last item, n `u32` each slot's item (0xFFFFFFFF for a
padding slot), then each item's `u64` index in item order. Keys tie often, so the order of pops
among equal keys is recorded; keys include −0, 0 and a NaN.

### IXEG

`EdgeGrid::Grid` (index-2). `str` the label; `u8` the `create` overload: 0 `Polygons`, 1
`std::vector<const Polygon *>`, 2 `std::vector<Points>` with open, 3 `Polygons` and `Polylines`, 4
one `ExPolygon` (the first), 5 `ExPolygons`; `bool` open (overload 2's argument); a `u64` count of
input paths, each `u8` role (0 polygon, 1 polyline, 2 an ExPolygon's contour, 3 a hole, 4 a list of
points), `u32` which ExPolygon, points. The paths make the containers in order: polygons, polylines
and lists in their own order, each ExPolygon its contour and its holes in order. `bool` a box set
first with `set_bbox`, and when true point min, point max, `bool` defined; `i64` the resolution.

Then the grid: point min, point max and `bool` defined of `bbox()`; `i64` `resolution()`, `u64`
`rows()`, `u64` `cols()`. A `u64` count of contours, each `u64` the input path it points into
(npos if none), `u64` the offset of its first point there, `u64` its point count, its points,
`bool` `open()`, `u64` `num_segments()`, then `get_segments()` as a `u64` count of lines, each
point a, point b, then for each segment i below `num_segments()` (no count: every contour `create`
keeps has at least one) `u64` `segment_idx_prev(i)`, `u64` `segment_idx_next(i)` and point
`segment_prev(i)`. Segment 0 of an open contour is past the engine's asserts there; the release
build gives the last index and the last point. A `u64` count of cells (rows × cols, row by row),
each `u64` begin and `u64` end into the cell data; a `u64` count of cell data, each `u64` contour
and `u64` segment.

Then a `u64` count of line visits, each point p1, point p2, `u64` stop, then the cells
`visit_cells_intersecting_line` visited: a `u64` count, each `i64` row and `i64` column. The
visitor stops the walk once it holds stop cells (0: never). The end points lie inside the cells.
Then a `u64` count of box visits, each point min, point max, `u64` stop, then the cells
`visit_cells_intersecting_box` visited the same way.

Then a `u64` count of points, each point, `i64` radius;
`closest_point_signed_distance(point, radius)` as `u64` contour, `u64` start point, `f64` distance,
`f64` t; `signed_distance_edges(point, radius, d, &on)` as `bool` its result, `f64` d (12345.5
before the call), `bool` on; `signed_distance(point, radius, d)` as `bool` its result, `f64` d
(12345.5 before). Then a `u64` count of `contours_simplified` calls, each `i64` offset, `bool`
fill_holes, then the polygons: a `u64` count, each its points.

Then `intersecting_edges()`: a `u64` count of pairs, each `u64` contour, `u64` segment, `u64`
contour, `u64` segment (a contour as its index in `contours()`; the engine's pointers sort in that
order). `bool` `has_intersecting_edges()`.

Then `bool` 1 (the field follows), `calculate_sdf()`'s field: a `u64` count ((rows + 1) × (cols +
1), row by row), each `f32`. Then for each point again `bool` and `f64` of `signed_distance` (now
falling back to the field) and `f32` `signed_distance_bilinear(point)`. Then for each
`contours_simplified` call again its polygons, which now read the field. Open contours get a field
too: the release build reads an open contour's last point as the one before its first.

Not recorded: `Grid::intersect` (every overload) and `Grid::inside` sit inside `#if 0` in
`EdgeGrid.hpp` and `EdgeGrid.cpp`, so they are not compiled and nothing in the engine calls them; a
port leaves them out. `Contour::segment_start`, `segment_end` and `get_segment` are recorded only
through what reads them (the cells, the distances, the field and `contours_simplified`); their only
other caller, `MultiMaterialSegmentation.cpp`, is painting, outside domain A.

The committed set holds every `create` overload, open contours (`open lists`, and the polylines of
`polygons and polylines`), contours that cross (the `bow tie`; `intersecting_edges` also finds
pairs in `spike`, `corners` and `polygons and polylines`) and a box set first (`box set first`,
`undefined box set first`). `box set first` has fewer queries (4 lines, 2 boxes, 5 points) so it
fits the limit. `two expolygons`, `pointers`, `open lists`, `polygons and polylines` and `bow tie`
pass it even so (the grid, its field and the simplified contours are most of a record), so each has
a coarse companion (`<label>, coarse`: the same paths on a grid four times coarser, with those
fewer queries) that the committed set holds. `corners`, `circle`, `slice` and `large` stay in the
full set only.

### IXEI

`Slic3r::intersecting_edges(const Polygons &)` (index-2), which checks support polygons: a `u64`
count of polygons, each its points; `bool` the result is empty; a `u64` count of pairs, each `u64`
segment and `u64` segment. The pairs' contours are pointers into the function's own grid, freed when
it returns, so they are not recorded; the order is the engine's (sorted by contour, then segment).

### IXNP

The inputs where `orca-index` stops with NotPorted (index-1 and index-2, each its own rows): the
release build reads past a vector's end, divides by zero, overflows a signed product or reads a
member never set. The engine never runs on them. `str`
the piece (`index-1` or `index-2`), `str` the functions, `str` the condition, then the example: a
`u64` count of `i64`. The examples' layouts:

- the priority queues: the IXPQ queue number, the items pushed, then the index or address;
- `EdgeGrid::Grid::create` and its queries: the resolution, then the polygon's points (x then y),
  then the query's points; the query on a grid never created has none;
- `Slic3r::intersecting_edges`: each polygon's point count, then its points.

A port answers every other input. index-1 has 5 rows and index-2 has 8.
