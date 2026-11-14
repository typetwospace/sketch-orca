# fill.sop1

The record kinds of the fill probe (`parity/oracle/probe/fill.cpp` and the sources of
`parity/oracle/probe/fill/`, phase O3): the infill of domain A, recorded by orca-o3-probe-fill
(2026-11-13) from the oracle's libslic3r. The values are those of `parity/SOP1.md` ("The
container", "Values" and "Probe files"). Every kind starts with `FL`, and no other page defines it
(`scripts/lib/parity/probe.ts`, `probeKindProblems`, which `test/parity-probe.test.ts` runs).
`orca-fill` replays the records: each section below names the piece whose port replays it (fill-1,
fill-2, fill-3 or fill-4), and the crate's probe test rebuilds every record of its pieces from the
Rust and compares it whole.

A record holds its inputs, then what the engine computed from them. Inputs are shapes the probe
builds (seeded with the splitmix64 of `parity/oracle/probe/probe.hpp`; the hand shapes are listed
below) and sections of OrcaSlicer's test meshes in `<exe>/../src/tests/data`
(`cube_with_concave_hole.obj`, `two_hollow_squares.obj`, `A.obj`, `frog_legs.obj`, `bridge.obj`,
`ipadstand.obj` and `extruder_idler.obj`, each read by `load_obj` and cut by `slice_mesh_ex` at 30,
55 and 80% of its height), never the corpus. The records run under the oracle's threading: one
thread in an arena of 8. The full set (`<work>/parity/probes/fill.sop1`) takes every case. The
committed set (`parity/probes/fill.sop1`, within 512 KB) takes, in the file's order, the hand-made
cases and then the first seeded ones (up to the kind's count, each of at most its size), each
while the kind's records there stay within the kind's share; a case that does not fit is left
out and the next ones are still tried, so the large cases (the biggest shapes, the most loops)
are in the full set only. Each kind's hand-made cases that reach a branch come first, on hand
shapes at half their size where a section says so.

## fill.sop1's values

`point`, `polygon`, `expolygon`, `surface`, `polyline` and `entity` are those of `parity/SOP1.md`
("Values"). Besides them:

- a thick polyline (`ThickPolyline`): `vec<point>`, `vec<fitting>`, `vec<f64>` widths (scaled, two
  for each segment: its start and its end), `bool` first end free, `bool` second end free;
- a box (`BoundingBox`): `point` min, `point` max, `bool` defined;
- a flow (`Flow`): `f32` width, `f32` height, `f32` spacing, `f32` nozzle diameter, `bool` bridge,
  which the replay builds the flow from;
- params (`FillParams`, in its order): `f32` density, `i32` multiline, `f32` anchor length, `f32`
  anchor length max, `f64` resolution, `bool` dont adjust, `bool` monotonic, `bool` complete,
  `bool` use arachne, `f64` layer height, `bool` gyroid optimized, `f64` lateral lattice angle 1,
  `f64` lateral lattice angle 2, `i32` pattern (`InfillPattern`), `f32` infill overhang angle, the
  flow, `u8` extrusion role (`ExtrusionRole`), `bool` using internal flow, `f32` no extrusion
  overlap, `bool` config (1 when `config` points at the record's configs, 0 when it is null),
  `bool` dont sort, `bool` can reverse, `f32` horiz move, `bool` symmetric infill y axis, `i64`
  symmetric y axis, `bool` locked zag, `f32` infill lock depth, `f32` skin infill depth;
- configs: `vec<f64>` the print's `nozzle_diameter`, `i32` the object's `gap_fill_target`
  (`GapFillTarget`), `f64` the region's `filter_out_gap_fill`. The three configurations
  (`PrintConfig`, `PrintObjectConfig`, `PrintRegionConfig`) are at their defaults with these three
  options set, and the params' config is the region's;
- a filler (`Fill`'s fields as set before the call): `u64` layer id, `f64` z, `f64` spacing, `f64`
  overlap, `f32` angle, `bool` fixed angle, `i64` link max length, `i64` loop clipping, the box
  `bounding_box`, `bool` dont alternate fill direction, `vec<expolygon>` no overlap expolygons,
  `bool` configs (1 when `print_config` and `print_object_config` point at the record's configs,
  0 when both are null);
- an outcome: `u8` 0 and then the call's results, or `u8` 1 and `str` the exception's `what()` in
  place of the results;
- a log: `str` what the engine logged at error or fatal during the call, as Boost.Log's text
  backend writes each record (`[error] ` and the message, then a line feed), "" when nothing;
- stdout: `str` what the call printed on stdout, byte for byte (`_infill_direction` prints
  `Using undefined infill angle` and a line feed when the angle is `FLT_MAX`).

Where a record's filler is the probe's own, it is a class of the probe that derives from `Fill`
and overrides only what is said here, so the record holds `Fill`'s own code: SpyFill (FLDF, FLDR,
FLSU) keeps the arguments of `_fill_surface_single` and fills nothing; GapFill (FLGP) and BoxFill
(FLEB) override nothing; StubFill (FLEX) returns given lines from `fill_surface` and
`fill_surface_arachne`, setting a given spacing first, or throws `InfillFailedException`.

The hand shapes, in order (millimetres; `hand_shapes` in `parity/oracle/probe/fill/common.cpp`): 0
a square of 8, 1 a rectangle of 14 by 5, 2 a square of 10 with a square hole of 4, 3 an L, 4 a comb
of five teeth, 5 a thin strip of 12 by 0.9, 6 a disc of radius 5 (64 sides), 7 an annulus of radii
6 and 3, 8 a seven-pointed star, 9 a dumbbell with a 0.6 neck, 10 a speck of 0.3, 11 a rectangle of
12 by 8 with a square and a round hole, 12 a turned square, 13 a triangle, and three of OrcaSlicer's
`tests/fff_print/test_fill.cpp`: 14 its diamond on the grid at a tenth of its size, 15 its diamond
and 16 its cut contour (its scaled points as they are). A seeded shape is a star (now and then with
a hole) or one of shapes 0 to 12, centred, scaled, turned and moved.

## The pieces and their edges

fill-1 replays FLDF, FLPA, FLAS, FLDR and FLSU: `FillParams` and `Fill`'s defaults, the anchors,
`_adjust_solid_spacing`, `_infill_direction` and `_layer_angle`, and `Fill`'s own `fill_surface`
and `fill_surface_arachne`. The static helpers of fill-1's lines behind `connect_infill` (the
contour walks, the boundary graph, `mark_boundary_segments_touching_infill`) use types of
`FillBase.cpp` alone and are reached only through `connect_infill`, so fill-2's FLCN holds them.
fill-2 replays FLCN, FLEB, FLGP and FLEX: `connect_infill`, `chain_or_connect_infill`,
`extended_object_bounding_box`, the gap fill and `fill_surface_extrusion`. Lines 1-1353 of
`FillRectilinear.cpp` (fill-2's) are static and reached only through
`FillRectilinear::fill_surface`, so fill-3's FLRP holds them. fill-3 replays FLRP: the rectilinear
family (rectilinear, monotonic, monotonicline) and crosshatch. fill-4 replays FLCP (concentric,
concentricinternal), FLNT, FLNM and FLRA. `group_fills`, `Layer::make_fills` and
`PrintObject::infill` take a whole layer, which the probe has not got: fill-4 replays them from the
goldens' posInfill dumps, and FLRP and FLCP set the filler and its parameters up as `make_fills`
does for each kind of surface.

A pattern record whose path leaves domain A is written with in domain A 0 and the engine's results
all the same: the port stops there with NotPorted naming the family `fill-multiline`
(`scripts/lib/parity/corpus.ts`). Such a path is `FillRectilinear::fill_surface` with multiline
above 1 and density at most 0.9999 (`fill_surface_by_multilines`), and `multiline_fill` past its
guard, which crosshatch and the concentric loops call with multiline above 1 (the concentric
Arachne branch at full density does not call it). FLNT marks the patterns outside domain A, whose
`new_from_type` a port answers with NotPorted naming `infill-<name>`.

### FLDF

`FillParams{}` and a new filler (fill-1): the params, `bool` `full_infill()`, `bool`
`dont_connect()`, `f32` `Fill::infill_anchor`, `f32` `Fill::infill_anchor_max`, the filler of a new
SpyFill (`Fill`'s constructor), and the `surface` `Surface()`. One record.

### FLPA

`FillParams::full_infill` and `dont_connect` (fill-1): `f32` density, `f32` anchor length max, then
`bool` `full_infill()`, `bool` `dont_connect()`. 13 pairs by hand (each threshold, 0.9999 and 0.05,
and the floats beside it, 0, -0, NaN and the infinities), then 400 random; the committed set takes
the first 200.

### FLAS

`Fill::_adjust_solid_spacing` (fill-1): `i64` width, `i64` distance, then `i64` the adjusted
distance. 21 pairs by hand (widths at and beside whole numbers of distances, the 20% limit, a
distance of 1, widths past 2^53 and at 2^62), then 600 random; the committed set takes the first
250.

### FLDR

`Fill::_infill_direction` and `Fill::_layer_angle` (fill-1), on SpyFill: the filler, the surface,
then `f32` the direction's angle, `point` its reference point, stdout, `f32` `_layer_angle(layer
id)` and `f32` `_layer_angle(layer id + 1)`. 25 cases by hand (layers 0 to 3, the angle `FLT_MAX`
with and without a box, an empty box and a box of no width, bridge angles of 0, -0, 1.2, 3.1, -1
and -0.5, layer id -1, a fixed angle, a direction not alternated, thicker surfaces, odd angles),
then 300 random; the committed set takes the first 120 of at most 400 bytes, within 20000 bytes.

### FLSU

`Fill::fill_surface` and `Fill::fill_surface_arachne` (fill-1), on SpyFill: `u8` the entry (0
`fill_surface`, 1 `fill_surface_arachne`), the filler, the surface, the params (`FillParams{}` with
`use_arachne` set for entry 1), stdout, then the calls of `_fill_surface_single` in order (a `u64`
count, each `u32` thickness layers, `f32` the direction's angle, `point` its reference point,
`expolygon` the region `offset_ex` gave), and `u64` the number of lines returned (0: SpyFill fills
nothing). 7 cases by hand (overlaps against half the spacing, spacings that cut the dumbbell's
neck, no spacing, an undefined angle printed once for each region, a negative overlap), every hand
shape with both entries, then 300 random (a fifth of them mesh sections); the committed set takes
the first 40 of at most 3000 bytes, within 35000 bytes.

### FLCN

`Fill::connect_infill` and `Fill::chain_or_connect_infill` (fill-2): `u8` the entry (0
`connect_infill` of an `ExPolygon`, 1 `connect_infill` of the boundary's `to_polygons` with the box,
2 `chain_or_connect_infill`), `expolygon` the boundary, the box (read by entry 1 only), `f64`
spacing, the params, `vec<polyline>` the infill in order (lines of a pattern clipped by the
boundary, some chained by `chain_polylines`, some with an end drawn in off the boundary, some
reversed), then the outcome with `vec<polyline>` the lines out. The params are `FillParams{}` with
density 0.2, the record's anchors and dont sort, and pattern crosshatch. 10 settings of entry,
anchors, sorting, chaining and drawn-in ends on shapes 2, 11 and 7, every hand shape the lines
cross with anchors of 1.8 and 20 mm, then 400 random (one with no lines and an
entry of 0 or 1 is left out) (a sixth of them mesh sections); the committed set takes the
first 40 of at most 6000 bytes, within 70000 bytes.

### FLEB

`Fill::extended_object_bounding_box` (fill-2), on BoxFill: the box `bounding_box`, then the box
returned. 5 boxes by hand (empty, the default, centred, off centre, far out), then 100 random; the
committed set takes them all.

### FLGP

`Fill::_create_gap_fill` (fill-2), on GapFill: the filler, the surface, the params, the configs,
`vec<polyline>` lines, `u8` role (`ExtrusionRole`), `f64` mm3 per mm, `f32` width, `f32` height
(the paths `extrusion_entities_append_paths` makes from the lines into the collection before the
call), then the outcome with `u64` the number of entities before the call and `vec<entity>` those
it appended (the call only appends), and the log. 14 cases by hand on hand shapes at half their size (lines 1.4 spacings apart on shapes 0, 2, 5, 7
and 9 with gaps to fill, the
target nowhere, an internal solid surface with the target top and bottom and everywhere, density
0.5, a bridge, no configs, a filter of 50 mm, no `no_overlap_expolygons`, no lines), then 200
random; the committed set takes the first 30 of at most 9000 bytes, within 40000 bytes.

### FLEX

`Fill::fill_surface_extrusion` (fill-2), on StubFill: the filler, `bool` what `no_sort()` answers,
`f64` the spacing the stub sets, `bool` whether it throws `InfillFailedException`,
`vec<polyline>` what `fill_surface` returns, `vec<thick polyline>` what `fill_surface_arachne`
returns, the surface, the params, the configs, then the outcome with `vec<entity>` the list the call
appends to (empty before), and the log. 14 cases by hand on hand shapes at half their size (solid
and sparse with the internal flow,
not reversible, not sorted, the stub throwing, no lines, Arachne's thick lines, a spacing of 0.1
that `Flow::with_spacing` refuses, an adjusted spacing, a bridge, a top surface, no configs, the
gap fill target top and bottom, a bottom surface not reversible), then 250 random; the committed
set takes the first 30 of at most 9000 bytes, within 55000 bytes.

### FLRP

The rectilinear family and crosshatch (fill-3): `u8` pattern (`InfillPattern`: 2 rectilinear, 0
monotonic, 1 monotonicline, 20 crosshatch), `bool` in domain A, the filler, the surface, the
params, the configs, `bool` extrusion; then a new filler of `Fill::new_from_type(pattern)` set as
the record's filler runs `fill_surface`: the outcome with `vec<polyline>` its lines, `f64` the
filler's spacing after it, the log; then, when extrusion is 1, another runs
`fill_surface_extrusion`, the call `make_fills` makes: the outcome with `vec<entity>` the list
(empty before), `f64` the spacing after it, the log. The surface,
filler and params are those `make_fills` gives one kind of surface: sparse (`stInternal`, internal
infill, the internal flow, densities from 5 to 95%, anchors of 400% of the spacing up to 20 mm or
others), internal solid, top and bottom (density 1, now and then below for top and bottom), bridge
and internal bridge (a bridging flow, `dont_adjust`, the bridge angle or none); a layer up to 200,
z, angles of 45, 0, 90, 30 or 135 degrees or any, the seam gap as loop clipping, the object box
around the surface. First, for each pattern, 8 cases by hand on hand shapes at half their size (no
anchors, unlimited anchors with a link limit, an overlap with extrusion but for crosshatch, the
speck at its size with extrusion, a full infill not adjusted, multiline 2 and 3 at 25% and multiline 2 at full
density), and crosshatch at densities 0.1, 0.29, 0.31, 0.998, 0.999 and 1; then every hand shape
under each of the four patterns, the kinds turning, extrusion on every third; then 600 random, the
patterns in turn (a fifth of them mesh sections of at most 40 mm, extrusion on a quarter); the
committed set takes the first 40 of at most 8000 bytes, within 160000 bytes.

### FLCP

Concentric and concentricinternal (fill-4): `u8` pattern (24 concentric, 29 concentricinternal),
`bool` in domain A, the filler, the surface, the params, the configs, `bool` extrusion; then a new
filler of `Fill::new_from_type(pattern)` runs `u8` the entry (0 `fill_surface`, 1
`fill_surface_arachne`, which it is when the params use Arachne): the outcome with
`vec<polyline>` or `vec<thick polyline>` its lines, `f64` the spacing after it, the log; then,
when extrusion is 1, another runs `fill_surface_extrusion` (concentricinternal's own): the outcome
with `vec<entity>` the list, `f64` the spacing after it, the log. The cases are set up as FLRP's,
with Arachne on (as `make_fills` sets it for both patterns). First, on hand shapes at half their
size: concentric's classic loops on 5 cases, multiline 2 with and without Arachne and at full
density, a full infill not adjusted with and without Arachne, no seam gap; concentricinternal with
no `no_overlap_expolygons` and with two. Then every hand shape under both: concentric with Arachne
(sparse, internal solid, top and bottom in turn, extrusion on every third) and concentricinternal
(internal solid, its loops in `no_overlap_expolygons`, extrusion always); then 240 random, the
patterns in turn (extrusion always for concentricinternal, on a quarter for concentric); the
committed set takes the first 30 of at most 6000 bytes, within 80000 bytes.

### FLNT

`Fill::new_from_type(InfillPattern)` and `Fill::use_bridge_flow` (fill-4), for every value from 0
to `ipCount`: `i32` the value, `bool` whether domain A has the pattern (monotonic, monotonicline,
rectilinear, crosshatch, concentric, concentricinternal), `str` its name in
`ConfigOptionEnum<InfillPattern>::get_enum_names()` ("" where the map has none: supportbase,
concentricinternal and `ipCount`), then the outcome with `bool` `use_bridge_flow()`, `bool`
`no_sort()`, `bool` `is_self_crossing()`, `bool` `has_consistent_pattern()`, `f32`
`_layer_angle` of 0, 1 and 7 with the angle not fixed, `f32` `_layer_angle` of 0 and 1 with it
fixed, and `bool` the static `Fill::use_bridge_flow(value)`. `ipCount` throws `unknown type`.

### FLNM

`Fill::new_from_type(const std::string &)` (fill-4): `str` the name, `bool` whether the pattern map
(`get_enum_values()`) has it, `i32` its value (-1 when not), `bool` whether a filler came back.
Every name of the map in its order, then 8 names it lacks (empty, capitalised, with a space,
concentricinternal, supportbase and three others).

### FLRA

`calculate_infill_rotation_angle` of `Fill.cpp` (fill-4), with no print object: `u64` layer id,
`f64` the fixed angle in degrees, `str` the template, then the outcome with `f64` the angle in
radians. The empty template (11 angles at layers 0, 1 and 57: `deg2rad` of the angle) and 10 plain
lists of angles (no character of the template language, read by `ConfigOptionFloats` and taken at
layer id modulo their count) at 7 layers. A template of the language reads the print object's
layers, which the probe has not got: fill-4 replays it from the posInfill dumps.
