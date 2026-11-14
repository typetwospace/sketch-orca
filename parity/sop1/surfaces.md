# surfaces.sop1

The record kinds of `surfaces.sop1`, the file of the surfaces probe
(`parity/oracle/probe/surfaces.cpp` and the sources of
`parity/oracle/probe/surfaces/`, phase O3): MutablePolygon, region expansion, and the helpers of the
surface steps that can be called on their own, recorded by orca-o3-probe-surfaces (2026-11-13) from
the oracle's libslic3r. The values are those of `parity/SOP1.md` ("The container", "Values" and
"Probe files"). Every kind starts with `SF`, and no other page defines it
(`scripts/lib/parity/probe.ts`, `probeKindProblems`, which `test/parity-probe.test.ts` runs).
`orca-surface` replays the records: each section below names the piece whose port replays it
(surfaces-1, surfaces-2 or surfaces-3), and the crate's probe test rebuilds every record of its
pieces from the Rust and compares it whole.

A record holds its inputs, then what the engine computed from them. Inputs are shapes the probe
builds (seeded with the splitmix64 of `parity/oracle/probe/probe.hpp`;
`parity/oracle/probe/surfaces/shapes.cpp` makes them) and the layers of OrcaSlicer's test meshes
(`<exe>/../src/tests/data`: `bridge.obj`, `overhang.obj`, `cube_with_hole.obj`,
`cube_with_concave_hole.obj`, `two_hollow_squares.obj`, `ipadstand.obj`, `sloping_hole.obj`,
`V.obj`, `A_upsidedown.obj`, `2x20x10.obj`, `A.obj`, `extruder_idler.obj` and `frog_legs.obj`),
never the corpus. A mesh is sliced by the engine's `slice_mesh_ex` every 0.2 mm from 0.1 mm above
its bottom, and each layer above the first is typed into fill surfaces by the probe's own rule (not
`detect_surfaces_type`): the slice shrunk by 1 mm is the fill; what of it is not over the layer
below, opened by 0.2 mm, the bottom bridges; what of the rest is not under the layer above, opened
by 0.2 mm, the tops; of what remains, the part within 2 mm of them the internal solid infill and the
rest the sparse infill, each surface 0.2 mm thick, in the order tops, bridges, solid, sparse. Of the
layers with a bottom bridge or a top, up to six of each mesh, spread over its height, are used: 24
layers of 11 meshes (`two_hollow_squares.obj` and `2x20x10.obj` are too thin to keep a fill). Every
input is written into its record, so a replay needs neither the meshes nor the rule. The records run
under the oracle's threading: one thread in an arena of 8.

The full set (`<work>/parity/probes/surfaces.sop1`) takes every record. The committed set
(`parity/probes/surfaces.sop1`, at most 512 KB) takes, of each kind, the first records up to a
count whose length is at most a size, in the file's order (the hand-made cases first); each section
gives its count and size.

## surfaces.sop1's values

`point`, `polygon`, `expolygon` and `surface` are those of `parity/SOP1.md` ("Values"): scaled `i64`
points, a `u64` count before each list (`vec<T>`), and a surface's `u32` type, `expolygon`, `f64`
thickness, `u16` thickness layers, `f64` bridge angle and `u16` extra perimeters. Besides them:

- `params`, a `RegionExpansionParameters` (`libslic3r/Algorithm/RegionExpansion.hpp`), its fields in
  order: `f32` tiny expansion, `f32` initial step, `f32` other step, `u64` number of other steps,
  `f32` max inflation, `f64` arc tolerance, `f64` shortest edge length;
- `seed`, a `WaveSeed`: `u32` source, `u32` boundary, `vec<point>` path;
- `expansion`, a `RegionExpansion`: `polygon`, `u32` source id, `u32` boundary id;
- `expansion-ex`, a `RegionExpansionEx`: `expolygon`, `u32` source id, `u32` boundary id;
- `zone`, the `ExpansionZone` of `libslic3r/LayerRegion.cpp` as made: `vec<expolygon>`, `params`; a
  zone after a call: `vec<expolygon>`, `bool` expanded into;
- `vec<u32>`: a `u64` count, then that many `u32`.

A permutation (`vec<u32>`) lists, for each place of a sorted list, the index in the unsorted list of
the item that lands there.

## The pieces and their edges

surfaces-1 replays SFMP, SFRD, SFSO, SFRP, SFRX and SFME: `libslic3r/MutablePolygon.hpp`,
`libslic3r/MutablePolygon.cpp`, `libslic3r/Algorithm/RegionExpansion.hpp` and
`libslic3r/Algorithm/RegionExpansion.cpp` whole. Their static helpers (`clip_narrow_corner`, the
expanded and opened seed contours, `merge_splits`, the AABB tree over the boundaries, the wavefront
steps) are reached through the public functions, as the engine reaches them. Domain A reaches
region expansion only through LayerRegion's bridge and surface expansion (SFBR records that use as
well), and never reaches MutablePolygon: `smooth_outward` is called by
`PrintObject::clip_fill_surfaces`, which returns at once since `PrintObject::infill_only_where_needed`
is false, and by the supports; the ExPolygons `remove_duplicates` by the multi-material
segmentation. The port carries all of it all the same.

surfaces-2 replays SFLP: `check_layer_id_pattern` (`libslic3r/utils.cpp`), which
`discover_horizontal_shells` calls for the `extra_solid_infills` pattern. Its other lines
(`PrintObject::detect_surfaces_type`, `discover_vertical_shells`, `clip_fill_surfaces`,
`discover_horizontal_shells`, `combine_infill`, `LayerRegion::prepare_fill_surfaces`,
`infill_area_threshold` and `trim_surfaces`) are members that read a print object and its layers,
which only `PrintObject` makes, and keep their helpers in lambdas: orca-o3-dumps' step dumps prove
them.

surfaces-3 replays SFBR and SFBF: the free functions of `libslic3r/LayerRegion.cpp` that
`LayerRegion::process_external_surfaces` calls (`expand_expolygons`, `group_id`,
`get_grouped_bridges`, `detect_bridge_directions`, `merge_bridges`,
`expand_bridges_detect_orientations` and `expand_merge_surfaces`; `fill_surfaces_extract_expolygons`
is static and reached through the last two). `process_external_surfaces` itself,
`PrintObject::prepare_infill`, `PrintObject::process_external_surfaces` and
`PrintObject::bridge_over_infill` (whose `gather_areas_w_depth`, `determine_bridging_angle` and
`construct_anchored_polygon` are lambdas) are proven by the step dumps. The probe declares
LayerRegion.cpp's `ExpansionZone`, `Bridge` and `ExpansionResult`, which no header has, as that file
defines them, and calls the engine's own compiled functions
(`parity/oracle/probe/surfaces/bridges.cpp`).

No function these records call reads a setting, so none reaches a family outside domain A, and no
record carries a NotPorted edge. The steps' edges are their own reads of the configuration, where
the port stops with NotPorted naming the family: `bridge_over_infill`'s branches for a
`sparse_infill_pattern` of `lightning` and its bridging-angle offsets for `hilbertcurve` and
`octagramspiral` (fill patterns outside domain A), `prepare_adaptive_infill_data`'s octrees for the
adaptive and support cubic patterns, and `detect_surfaces_type`'s reading of `has_support()`
(supports).

### SFMP

A script of MutablePolygon operations (surfaces-1). `u8` how the polygon is made: 0
`assign(points.begin(), points.end(), reserve)` on a default polygon; 1 `vec<point>` other points
first assigned with no reserve, then `assign(Polygon(points), reserve)`; 2
`MutablePolygon(Polygon(points), reserve)` copied over a default one. Then `vec<point>` points,
`u64` reserve, the state, `u32` the number of steps, and each step: `u8` the operation, its
arguments, the state after it. A cursor `it` starts at `begin()`; a range of unprocessed points may
be held.

The state: `u64` size, `bool` empty, `bool` valid (at least 3 points), `bool` the cursor is valid;
if it is, its `point`, `bool` it equals `begin()`, `bool` it equals `end()` (the last point before
the roll over, not one past it); `bool` a range is held; if one is, `bool` it is empty, and when it
is not, its begin's `point`, its end's `point`, `bool` its begin is the cursor, `bool` its end is
the cursor.

The operations: 0 `++it`; 1 `--it`; 2 `point` q, `it = it.insert(q)`; 3 `it.remove()`; 4
`it = begin()`; 5 `it = end()`; 6 `vec<point>` the polygon (`polygon()`, empty under 3 points); 7
`clear()`, `it = begin()`, the range dropped; 8 `vec<point>` q, `u64` reserve,
`assign(q.begin(), q.end(), reserve)`, `it = begin()`, the range dropped; 9 `vec<point>` q, `bool`
`==` and `bool` `!=` against `MutablePolygon(Polygon(q))`; 10 a range over the whole polygon
(`range(polygon)`), replacing any; 11 `it = range.process_next()`; 12 `range.advance_front()`; 13
`range.retract_back()`; 14 `it = range.remove_front(it)`; 15 `it = range.remove_back(it)`; 16
`point` q, `*it = q`. A script takes only steps the class allows: 0 to 3 and 16 with a valid
cursor, 3 never on a range's end point, 10 on a polygon that is not empty, 11 to 13 on a range that
is not empty, 14 and 15 with a valid cursor and a range, 14 never on the range's end unless it is
also its begin and 15 never on its begin unless it is also its end. 2000 seeded scripts, of 4 to 33
steps (the first 200) and 4 to 83 steps, points within 2^4 to 2^27 of the origin, duplicates on
purpose; the committed set the first 50 of at most 1200 bytes.

### SFRD

The `remove_duplicates` of `libslic3r/MutablePolygon.hpp` (surfaces-1). `u8` the overload: 0
`remove_duplicates(MutablePolygon&)`, 1 `(MutablePolygon&, double eps)`, 2 `(MutablePolygon&,
coord_t scaled_eps, double max_angle)`, 3 `(ExPolygons, coord_t scaled_eps, double max_angle)`. For
0 to 2: `polygon` in, then `f64` eps (1) or `i64` scaled eps and `f64` max angle (2), then `polygon`
the result (`polygon()` of the MutablePolygon, empty under 3 points). For 3: `vec<expolygon>` in,
`i64` scaled eps, `f64` max angle, `vec<expolygon>` the result. 8 polygons by hand (empty, one
point, two and four equal points, a square with duplicates at the seam, a spike within eps, a
collinear run, a near duplicate at the end), each through 0, through 1 with eps 3 and 10, and
through 2 with (10000, pi / 6), (5, 0) and (10, pi / 2); two ExPolygons lists by hand (none; a
square with a tiny hole and a square hole, beside a tiny triangle); then 1500 seeded noisy polygons
and lists, a third of the calls of 2 and 3 with MultiMaterialSegmentation's scaled 0.01 mm and
pi / 6; the committed set the first 80 of at most 1500 bytes.

### SFSO

`smooth_outward` (surfaces-1), with `clip_narrow_corner` inside it. `u8` the overload (0 a
`Polygon`, 1 `Polygons`, 2 `ExPolygons`), `i64` the clip distance, the input (`polygon`,
`vec<polygon>` or `vec<expolygon>`), then the output of that overload in the same form. 17 polygons
by hand (a square, cracks narrower and wider than the clip distance, a slit widening at its end, a
slit blocked on one side, tiny clockwise triangles low and tall, a counter-clockwise one, obtuse
concave corners, a clockwise square and crack, a spike and a narrow wedge), each at the clip
distances 100000 (`clip_fill_surfaces`'s scaled 0.1 mm), 500000 and 20000; no polygons, and the 17
as one list; no expolygons, and one with a cracked and a tiny hole; then 1500 seeded combs, noisy
polygons, lists of them, and `closing_ex` regions as `clip_fill_surfaces` makes them; the committed
set the first 90 of at most 2500 bytes.

### SFRP

`RegionExpansionParameters::build` (surfaces-1): `f32` the full expansion, `f32` the step, `u64` the
most steps, then `params`. The seven expansions `process_external_surfaces` asks for with a 0.4 mm
nozzle (the perimeter spacing 407080, `SCALED_EPSILON` with no walls, and the shell width times
sqrt 2 for one to five walls), by scaled 0.1 mm in at most 5 steps; then a grid of 12 expansions, 5
steps and 4 counts around each branch (the tiny expansion a quarter or 0.05 mm, one step, the step
count capped); then 2000 seeded ones, log-uniform; the committed set the first 260 (every record is
56 bytes).

### SFRX

One source and boundary through every public function of `libslic3r/Algorithm/RegionExpansion.hpp`
(surfaces-1). `str` a label, `vec<expolygon>` source, `vec<expolygon>` boundary, `f32` expansion,
`f32` step, `u64` most steps, `params` (`build` of the three); then:

1. `vec<seed>` `wave_seeds(source, boundary, tiny expansion, false)`;
2. the sorted seeds (`wave_seeds(..., true)`): `u8` 0 and `vec<u32>` the permutation `std::sort`
   with `lower_by_boundary_and_src` makes of the unsorted ones, or `u8` 1 and `vec<seed>` the sorted
   seeds whole when that permutation does not give them;
3. `vec<expansion>` `propagate_waves(sorted seeds, boundary, params)`;
4. `vec<expansion-ex>` `propagate_waves_ex(sorted seeds, boundary, params)`;
5. `bool` `expand_expolygons(source, boundary, expansion, step, most steps)` equals 3 grouped by
   source id in order; if not, `u64` a count and each `vec<polygon>`;
6. `vec<expolygon>` `merge_expansions_into_expolygons(source, 3)`;
7. four `bool`s, each followed by the result when it is 0: `propagate_waves(source, boundary,
   params)` equals 3 (`vec<expansion>`), `propagate_waves(source, boundary, expansion, step, most
   steps)` equals 3 (`vec<expansion>`), `propagate_waves_ex(source, boundary, expansion, step, most
   steps)` equals 4 (`vec<expansion-ex>`), `expand_merge_expolygons(source, boundary, params)` equals
   6 (`vec<expolygon>`).

The layouts by hand reach each branch of `wave_seeds` and of the propagation: touching side by
side, a source filling the boundary's hole (a closed seed the AABB tree finds), two boundaries, a
boundary in the source's hole, two sources and two boundaries, a gap, no source, no boundary, an
island in the boundary's hole, a corridor narrower than a step, touching at a corner, overlapping, a
long thin source, a star into a ring, and boundaries on the vertices of the expanded seed contour (a
diamond around its corner, a triangle on two of its vertices, a notch touching one, a hole that is
the expanded contour itself); each at the two-wall bridge expansion 1103637 and again at 407080
(into sparse infill), with three more (100, one step, 3 mm by 0.5 mm). Then each mesh layer's
bridges into its shells, its sparse infill and its tops, and its tops into its shells; then 500
seeded layouts whose source and boundary share edges; the committed set the first 70 of at most
6000 bytes.

### SFME

`merge_expansions_into_expolygons` of expansions made here (surfaces-1): `vec<expolygon>` source,
`vec<expansion>` expansions, `vec<expolygon>` the result. Seven by hand (no expansions, one, four
out of source order, one apart from its source so that the union has two parts, a source of no area
dropped, an expansion of no area, a hole filled by its expansion), then 500 seeded (one to five
sources, a few flat, up to eight expansions, each with a source id and a boundary id); the committed
set the first 45 of at most 3000 bytes. Its `std::sort` by source id alone is not stable, so ties
come out in the order MSVC's `std::sort` leaves them.

### SFLP

`check_layer_id_pattern` (surfaces-2): `str` the pattern, `i32` the first layer index (-2), `u32`
the number of indices asked, then each index's `i32` and `bool` the answer: the indices -2 to 60,
then 2147483646, -2147483648, -2147483647, 99, 100, 1000 and 65535 (2147483647 is left out: the
function adds 1 to the index). 67 patterns by hand (the forms `N`, `N#K`, `N#`, `#K` and lists;
signs, zero and negative steps, white space `std::stoi` skips and the function's filter keeps, `\v`
and `\f`, text after the digits, hexadecimal and exponent forms, quotes, numbers past `int`, empty
tokens, a UTF-8 no-break space, repeated `#`), then 300 seeded from the pieces the parser reads; the
committed set the first 100 of at most 1024 bytes.

### SFBR

One layer's fill surfaces through `expand_bridges_detect_orientations`, step by step, and through
`expand_merge_surfaces` (surfaces-3). The inputs: `str` a label, `vec<surface>` the fill surfaces,
`u64` the zone count (3) and each `zone` (shells: the union of the internal solid surfaces,
expanded into by the bridge expansion; sparse: the union of the internal ones, by the perimeter
spacing; tops: the union of the tops, by the bridge expansion; each by scaled 0.1 mm in at most 5
steps, as `process_external_surfaces` builds them), `f32` the closing radius (0.55 x 0.65 x 1.05 x
the solid infill spacing), `f64` a bridge angle. Then:

1. `bool` there are bottom bridges; if there are, the steps of `expand_bridges_detect_orientations`
   in its order, on copies: `vec<seed>` the anchors and `vec<expansion-ex>` the expansions of
   LayerRegion.cpp's `expand_expolygons(bridges, zones)`, then a `bool` expanded into for each zone
   (no count); the bridges of `get_grouped_bridges`: `u64` their count, each raw `u32` group id,
   then each `u32` `group_id(bridges, i)` in order on a copy; `vec<u32>` the permutation of the
   anchors' `std::sort` by `lower_by_src_and_boundary`; the angles of `detect_bridge_directions` on
   the sorted anchors: `u64` a count, each `bool` set and its `f64`; `vec<u32>` the permutation of
   the expansions' `std::sort` by source id then boundary id; `vec<surface>` `merge_bridges` of
   them;
2. `expand_bridges_detect_orientations(surfaces, zones, closing radius)` on fresh copies: `bool` it
   equals the result of 1 (empty when there are no bridges), and when it does not, `vec<surface>`
   it; the zones after it (`u64` a count, each zone after); `u64` the surface count and each `bool`
   the call moved the surface's expolygon out (it is now empty);
3. `expand_merge_surfaces(surfaces, stBottomBridge, zones, closing radius, bridge angle)` on fresh
   copies: `vec<surface>`, the zones after, the moved flags as in 2;
4. the same for `stBottom` and then `stTop`, with the angle -1 and the first two zones only (as
   `process_external_surfaces` calls them once the tops' zone is popped).

The probe checks that `std::sort` on the anchors and on the expansions gives the permutation it
writes, and fails otherwise. 14 layouts by hand (a bridge between two shells, two bridges across a
narrow shell in one group, two far apart, a bridge in a shell's hole, isolated bridges, a bridge
touching all three zones, one over sparse infill only, no bridge with bottoms and tops, an L
anchored on two sides, a bridge with a hole over a shell island, three chained by overlapping
expansions, no walls, five walls); then each mesh layer, at the angle 0.5; then 300 seeded layers (a
fifth with bottoms for bridges, zero to five walls, now and then another solid spacing); the
committed set the first 30 of at most 12000 bytes.

### SFBF

`get_grouped_bridges`, `detect_bridge_directions` and `merge_bridges` alone, on inputs made here
(surfaces-3). `u8` the function:

- 0 `get_grouped_bridges`: `vec<expolygon>` the bridges, `vec<expansion-ex>` the expansions; then
  `u64` the bridge count, each raw `u32` group id, and each `u32` `group_id(bridges, i)` in order on
  a copy;
- 1 `detect_bridge_directions`: `vec<seed>` the anchors, `vec<expolygon>` the bridges (bridge i in
  group i), `u64` the zone count and each zone's `vec<expolygon>` (its parameters are not read);
  then `u8` 1 and the angles (`u64` a count, each `bool` set and its `f64`), or `u8` 2 and `str` the
  exception's `what()` (with no zone, "At least one expansion zone must exist!");
- 2 `merge_bridges`: `u64` the bridge count and each bridge's `expolygon`, `u32` group id (never
  above its own index), `bool` an angle is set and its `f64`; `vec<expansion-ex>` the expansions,
  sorted by source id then boundary id (every bridge's `bridge_expansion_begin` starts at their
  `end()`); `f32` the closing radius; then `vec<surface>` the result (an unset angle gives -1).

By hand: grouping with no expansions, overlapping ones in one boundary and in two, a chain of three,
out of order, a duplicate, and bounding boxes that overlap while the contours do not; orientation
with no zone, no anchors, a square and a tiny triangle with an empty zone, anchors on one side, on
opposite sides in two zones, in the second zone only, past every zone, and for the second bridge
only; merging a group of two with angles, unset angles, a bridge with no expansion, and a closing
radius of 1. Then 600 seeded, a third of each function, of one to six bridges with up to three
expansions each on their sides; the committed set the first 70 of at most 2500 bytes.
