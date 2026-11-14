# walls.sop1

The record kinds of the walls probe (`parity/oracle/probe/walls.cpp` and the sources of
`parity/oracle/probe/walls/`, phase O3): PerimeterGenerator, classic and Arachne, BridgeDetector
and the principal components, recorded by orca-o3-probe-walls (2026-11-13) from the oracle's
libslic3r. The values are those of `parity/SOP1.md` ("The container", "Values" and "Probe files").
Every kind starts with `WL`, and no other page defines it (`scripts/lib/parity/probe.ts`,
`probeKindProblems`, which `test/parity-probe.test.ts` runs). `orca-perimeter` replays the records:
each section below names the piece whose port replays it (walls-1, walls-2 or walls-3), and the
crate's probe test rebuilds every record of its pieces from the Rust and compares it whole.

A record holds its inputs, then what the engine computed from them. Inputs are shapes the probe
builds (seeded with the splitmix64 of `parity/oracle/probe/probe.hpp`) and the OBJ meshes of
OrcaSlicer's tests (`<exe>/../src/tests/data`), never the corpus. The records run under the
oracle's threading: one thread in an arena of 8. The committed set (`parity/probes/walls.sop1`)
takes the records the probe marks, in the file's order, and they must all fit 500 KB (the probe
fails when one does not); the full set (`<work>/parity/probes/walls.sop1`) takes every record.

## walls.sop1's values

`point`, `polygon`, `expolygon`, `surface`, `polyline` and `entity` are those of `parity/SOP1.md`
("Values"): scaled `i64` points, a `u64` count before each list, an entity by its exact type with
its path, loop, multi-path or collection fields (a path's points `i64` x, y, z). Besides them:

- a flow: `f32` width, `f32` height, `f32` spacing, `f32` nozzle diameter, `bool` bridge (the five
  fields of `Flow`, which the replay builds the flow from);
- settings: a `u64` count, each `str` key and `str` value. The three configurations a record's
  settings make are `PrintRegionConfig`, `PrintObjectConfig` and `PrintConfig` at their defaults,
  each then `apply`ed (options it has not got ignored) from a `DynamicPrintConfig` with every
  setting set from its text by `set_deserialize_strict`, in order;
- a line: `point` a, `point` b; lines: a `u64` count of them;
- a 3D path (`ClipperLib_Z::Path`): a `u64` count, each point `i64` x, y and z (z the width);
- an Arachne line (`Arachne::ExtrusionLine`): `u64` inset index, `bool` odd, `bool` closed, a `u64`
  count of junctions, each `i64` x, `i64` y, `i64` width and `u64` perimeter index;
- a `Vec2f` is two `f32`, a `Vec2d` two `f64`.

A record whose call threw writes `u8` 2 and `str` the exception's `what()` where its result would
be, and ends there; `u8` 1 comes before a result.

## The pieces and their edges

walls-1 replays WLMT, WLPC, WLDB, WLBD, WLEA, WLRC, WLPT, WLSX and WLXP: the functions of its lines
that can be called on their own. Its lines' static functions (`detect_steep_overhang`,
`traverse_loops`, `clip_extrusion` with its ClipperLib_Z fill of the widths, `traverse_extrusions`,
`reorient_perimeters`) and private ones (`split_top_surfaces`, `apply_extra_perimeters`) are reached
only through the two processes, so walls-2's WLCL and walls-3's WLAR hold them: the overhang widths
`clip_extrusion` interpolates come out in WLAR's overhang paths. walls-2 replays WLCL
(`process_classic`, `process_no_bridge`, `generate_lower_polygons_series`). walls-3 replays WLAR
(`process_arachne`), WLTP and WLCP. `Layer::make_perimeters`, `LayerRegion::make_perimeters` and
`PrintObject::make_perimeters` read a print object, which the probe has not got: walls-3 replays them
from the goldens' object dumps after posPerimeters, and WLCL and WLAR set the generator up as
`LayerRegion::make_perimeters` does.

A perimeter job whose settings reach a family outside domain A is written with that family (its
`str` edge) and no result: the port stops there with NotPorted naming it. The families: `fuzzy-skin`
(a region's `fuzzy_skin` other than `none` and `disabled_fuzzy`) and `raft` (`raft_layers` above 0).

### WLMT

`compute_moments_of_area_of_triangle` (walls-1): `f32` a x, a y, b x, b y, c x, c y; then `f32` the
area, the first moment of area x and y, the second moment of area x and y, and the second moment's
covariance. 10 triangles by hand (degenerate, collinear, tiny, far from the origin), then 300 random
in boxes of 150, 10 and 0.5 mm; the committed set the hand-made ones and the first 30.

### WLPC

`compute_principal_components` (walls-1): `vec<polygon>`, then `f32` the first component's x and y
and the second's. 17 lists by hand (none, a square and rectangles whose covariance is under EPSILON,
rotated rectangles, a clockwise square whose area is not above 0, a square with a clockwise hole, a
triangle, an L, a polygon of 1 by 1 units, two polygons, a circle, two points), then 120 random lists
of one to three stars, now and then the last reversed; the committed set the hand-made ones and the
first 16.

### WLDB

`detect_bridging_direction` (walls-1): `u8` the overload, then for 0 (`const Lines &floating_edges,
const Polygons &overhang_area`) the lines and `vec<polygon>` the overhang's area, for 1 (`const
Polygons &to_cover, const Polygons &anchors_area`, which calls overload 0) `vec<polygon>` to_cover
and `vec<polygon>` anchors_area; then the `Vec2d` direction and `f64` the cost. Overload 0 keeps
each edge's normal under the key `ceil(atan2 * 1000)` of an `std::unordered_map<double, Vec2d>`
and walks the map in its order: the replay holds MSVC's. 21 cases of overload 1 by hand (an
overhang anchored on one to four sides, on none, one too small to have a second component, a
circle, an L, nothing to cover; a bridge and a one-sided overhang at six angles), 6 of overload 0
(no edges, one, two, two whose normals take one key), then 80 random of overload 0 and 60 of
overload 1; the committed set the hand-made ones but three, and the first 12 and 8 random.

### WLBD

`BridgeDetector` (walls-1): `u8` the constructor (0 the one of an `ExPolygon`, given the first of the
list; 1 the one of `ExPolygons`), `vec<expolygon>` the bridge's expolygons, `vec<expolygon>` the
lower slices, `i64` the spacing, `f64` the override `detect_angle` is given, `f64` a second angle;
then `u8` 1, `f64` `resolution`, `bool` `detect_angle(override)`, `f64` `angle`, `vec<polygon>`
`coverage(-1, true)`, `coverage(-1, false)` and `coverage(second, true)`, `vec<polyline>`
`unsupported_edges(-1)` and `unsupported_edges(second)`. These reach the private
`bridge_direction_candidates` and `initialize` and the file's static `get_trapezoids2` (not precise)
and `get_trapezoids3_half` (precise). 18 cases by hand (a bridge between two pads with the spacing
and with a quarter of it as `process_no_bridge` gives it, overrides of PI/2, 3.5, 0.01 and PI, three
rotations, a U-shaped support, a counterbore's ring, no support, a support far away, an L, two
bridges, a bridge with a hole, one pad), then 40 random rotated bridges; the committed set the first
26.

### WLEA

`extrusion_paths_append` of `Arachne/utils/ExtrusionLine.cpp` (walls-1): `u8` the overload (0 of
`ClipperLib_Z::Paths`, 1 of an `Arachne::ExtrusionLine`), `u8` the role (`ExtrusionRole`), the flow,
then the 3D paths (a `u64` count of them) or the Arachne line; then `u8` 1 and `vec<entity>` the
paths appended to an empty list. The paths are `thick_polyline_to_multi_path`'s, with a tolerance
of 0.05 mm and a merge tolerance of `SCALED_EPSILON`. 7 cases by hand (a two-point path with each
kind of flow, no paths, segments under `SCALED_EPSILON` at the start, inside and at the end, widths
that step past the tolerance), 160 random lists of paths and 120 random Arachne lines (closed and
open, widths constant, varying within the tolerance, past it or jumping, points repeated); the
committed set the hand-made ones and the first 8 and 12.

### WLRC

`reconnect_polylines` (walls-1): `vec<polyline>` the polylines, `f64` the limit distance, then
`vec<polyline>` the result. The result is the walk of an `std::unordered_map<size_t, Polyline>`, in
MSVC's order. 11 cases by hand (none, one, each of the four joins, a chain, a ring, an empty polyline
among them, none close, a limit of 0), then 120 random lists whose ends now and then start next to
another's; the committed set the hand-made ones and the first 20.

### WLPT

`paths_touch` (walls-1): `entity` the first path, `entity` the second (overhang perimeters of a 0.45
mm flow), `f64` the limit distance, then `bool` the result. 5 cases by hand, then 100 random; the
committed set the hand-made ones and the first 24.

### WLSX

`sort_extra_perimeters` (walls-1): `vec<entity>` the paths, `i32` the index of the first unanchored,
`f64` the spacing, then `vec<entity>` the result. It keeps its dependencies in
`std::unordered_set<size_t>`s, read only by membership and emptiness. 6 cases by hand (none, three
parallel lines anchored to different depths, short paths the filter drops, ends close enough to join,
a path past 5 mm), then 80 random; the committed set the hand-made ones and the first 14.

### WLXP

`generate_extra_perimeters_over_overhangs` (walls-1): `vec<expolygon>` the infill area,
`vec<polygon>` the lower slices' polygons (grown by half the nozzle, as the processes grow them),
`i32` the perimeter count, the overhang flow, `f64` the scaled resolution (the object and print
configurations at their defaults: it reads neither); then `u8` 1, a `u64` count of overhang regions,
each `vec<entity>` its paths, and `vec<polygon>` the area they fill. The function keeps OrcaSlicer's
`if (double d = ... < min_dist)`, which assigns the comparison to d. 10 cases by hand (no overhang, a
strip over the air with one to three perimeters and both flows, a bridge between pads, a corner
anchor, a narrow overhang that takes the gap fill, a circle, a square with a hole, no lower slices),
then 30 random rotated; the committed set the hand-made ones and the first random one.

### WLTP

The inner-outer-inner reordering of Arachne's walls (walls-3): `u8` the call, a `u64` count of
entries, each an Arachne line and `bool` `is_contour`, then:

- 0, `findAllTouchingPerimeters`: a `u64` count and each `i32` reference, in the order they are
  inserted into the `std::unordered_set<int>`; `u64` the external threshold, `u64` the internal
  threshold, `u64` the inset index looked for; then a `u64` count and each `i32` index found, in the
  order returned (the walk of an `std::unordered_set<int>`, MSVC's);
- 1, `reorderPerimetersByProximity`: `u64` the external threshold, `u64` the internal threshold; then
  a `u64` count and each entry of the result as `u64` its line's index among the entries and `bool`
  `is_contour`;
- 2, `bringContoursToFront`: the entries after it, written as call 1 writes its result.

The entries are the walls `Arachne::WallToolPaths` makes for 8 shapes (squares, holes, islands, an
L, a strip, a triangle; one to five insets; the default object settings; bead widths of 0.42 and
0.45 mm flows' spacings), in the order of their perimeters, then the same reversed. Call 0 runs from
each line of inset 0 or 1 (with the next line too when that is of inset 1), for insets 1 and 2.
Thresholds: half the external spacing and half the internal one, the external spacing and half the
internal one (the precise outer wall's), and the internal spacing. The committed set takes the
first order of 2 shapes (the 10 mm square and the 1.3 mm strip), call 0 from their first two lines
only.

### WLCP

`Layer::is_perimeter_compatible` (walls-3): settings a, settings b, then `bool` the result for the
`PrintRegion`s of their region configurations. 2 cases of equal settings, then a base with each of
the 33 settings the check compares changed, 6 it leaves out and 2 written in another text for the
same value; all in the committed set.

### WLCL

`PerimeterGenerator::process_classic` of a perimeter job (walls-2). The job: `str` a label, the
settings, `i32` the layer id, `f64` the layer height, `f64` the slice z (the layer id times the
height and half the height), `bool` spiral mode, `f64` the model's rotation in radians; then `u8` 2
and `str` `what()` when no flow can be made for the settings, which ends the record, or `u8` 1 and
the four flows (perimeter, external perimeter, overhang, solid infill), `vec<surface>` the slices,
`bool` and `vec<expolygon>` the lower slices (`lower_slices` is null when false), `bool` and
`vec<expolygon>` the upper slices, `vec<surface>` the upper layer's slices of the same region (given
only with the upper slices; `upper_slices_same_region` is null without them), `str` the edge. With an
edge, `u8` 0 ends the record. Else `u8` 1 and the generator's outputs: `entity` the loops (the
collection given as `loops`), `entity` the gap fill, `vec<surface>` the fill surfaces,
`vec<expolygon>` the fill without overlap, `f64` `ext_mm3_per_mm()`, `mm3_per_mm()`,
`mm3_per_mm_overhang()` and `smaller_width_ext_mm3_per_mm()`, the flow
`smaller_ext_perimeter_flow`, `vec<polygon>` `lower_slices_polygons()`, then
`m_lower_polygons_series`, `m_external_lower_polygons_series` and
`m_smaller_external_lower_polygons_series`, each a `u64` count of `vec<polygon>`, `bool`
`has_fuzzy_skin`, `bool` `has_fuzzy_hole`, `u64` the size of `regions_by_fuzzify`.

The generator is made as `LayerRegion::make_perimeters` makes it: the slices, one compatible region
(a `LayerRegion` of the region configuration whose slices are the job's), the layer height, the
slice z, the perimeter flow, the three configurations, spiral mode and the rotation; then the lower
and upper slices, the layer id and the other three flows. The flows are the region's
(`PrintRegion::flow` of each role with the nozzle of `outer_wall_filament_id`, the first layer's
line width on layer 0) and the perimeters' bridging flow (`LayerRegion::bridging_flow` with the
object's `thick_bridges`). Every job's settings start with line widths of 0.42 mm (outer walls,
solid infill, the default) and 0.45 mm (inner walls, sparse infill). The jobs: 71 layers by hand,
each made for a branch of `process_classic` or of what it calls (the first layer with one wall, an
outer brim and a wider line; holes, islands, thin strips with thin walls detected or not; a
triangle; three narrow strips whose gap fill is 3.1, 10.3 and 14.1 mm long, with gap fill, without,
with `filter_out_gap_fill` at 6 mm (it drops the short strip's piece and keeps the two others) and
without sparse infill (the 1.8 mm strip's gap is found only by the run after the last wall, which
then stops early, so its piece goes and the two others stay); an L whose one gap-fill piece, 27.9
mm, a 30 mm filter drops; overhangs reversed at each threshold, on odd and even layers, internal
only, undetected, with extra perimeters and thick bridges; bridges; counterbores bridged partially
and with a sacrificial layer, at an angle, relative, aligned to the model, around an island, over
nothing; top surfaces with one wall, interface shells, the topmost layer, over an overhang, a 1 mm
exposed strip that the default `min_width_top_surface` (300%) drops and 50% keeps, everywhere; each
wall sequence, precise outer walls or not, clockwise walls, arc fitting, an alternating extra wall,
none to six walls, a coarser resolution, a 0.6 mm nozzle at 0.3 mm, automatic line widths, a
surface's extra perimeters, a bottom bridge surface; spiral vase on two islands whose external
offset splits in two, an 8 mm and a 6 mm square joined by a 0.3 mm neck and two 0.6 mm strips joined
by a 0.2 mm neck, narrow enough for the smaller external width (`keep_largest_contour_only` of the
offsets, then of the narrow offsets), each beside its twin without spiral vase, which keeps both
loops; spiral vase turning off the extra perimeters over overhangs and the alternating extra wall),
3 edges (fuzzy skin on the outer walls and on every wall, a raft), then layers of 14 meshes of
OrcaSlicer's tests, each with the defaults and with three walls, overhangs reversed, extra perimeters
and one wall on top.

A branch is recorded on both its sides, so on 13 hand-made layers the setting changes nothing there
and WLCL holds the same outputs as for its twin: three walls and Arachne's bead settings on the thin
strips (as the strips without thin walls), thin walls on the L (as the L), the L's 30 mm filter (as
the L without gap fill), the overhang reversed at threshold 0 (as on an odd layer), reversed on an
even layer and spiral vase with extra perimeters over the overhang (as the overhang on one side),
extra perimeters on the bridge (as the bridge), the sacrificial layer at 30 degrees and the relative
angle (as their counterbores at no angle), the 1 mm top strip at the default minimum (as three walls
with no top surface: the strip is dropped), the alternating extra wall without sparse infill and
spiral vase's alternating extra wall (as the square of a middle layer). A port that took the other
side of such a branch writes different outputs.

The committed set takes the hand-made jobs but 24 (larger layers, holes and circles of many
segments, whose branches a committed job reaches too, and repeats of a branch: `full_only` in
`walls/jobs.cpp`) and the 20 mm cube's layers 0 and 50 with the defaults: 52 of the 158 jobs (74
by hand, 84 of meshes).

### WLAR

`PerimeterGenerator::process_arachne` of a perimeter job (walls-3): the job and the outputs as WLCL
writes them, of the same jobs but the spiral vase ones: `LayerRegion::make_perimeters` runs
`process_classic` in spiral mode whatever `wall_generator` says, so `process_arachne` never gets a
spiral job: 154 jobs, 48 of them committed (WLCL's committed jobs but the 4 of spiral vase).
`smaller_ext_perimeter_flow` and the three series are left as the constructor leaves them
(process_arachne sets neither). The paths of overhanging walls carry the widths `clip_extrusion`
gives the points Clipper makes (the ClipperLib_Z fill: the width interpolated along the subject's
edge, or projected onto the subject for a point of the clip).
