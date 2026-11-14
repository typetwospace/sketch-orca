# layer.sop1

The record kinds of the layer probe (`parity/oracle/probe/layer.cpp`, phase O3): the static print
configs, extrusion entities, Flow, variable width, Surface, Layer and its regions. Every kind
starts with `LY`, and no other page defines it (`scripts/lib/parity/probe.ts`, `probeKindProblems`,
which `test/parity-probe.test.ts` runs). `orca-config` replays the front-1 records and `orca-layer`
the layer-1, layer-2 and layer-3 records; each section below names its piece. Recorded by
orca-o3-probe-layer, 2026-11-13.

The sources are `parity/oracle/probe/layer/config.cpp` (front-1),
`parity/oracle/probe/layer/flow.cpp` and `parity/oracle/probe/layer/extrusion.cpp` (layer-1, and
LYCH of layer-3), `parity/oracle/probe/layer/surface.cpp` (layer-2) and
`parity/oracle/probe/layer/print.cpp` (layer-3), with the writers they share in
`parity/oracle/probe/layer/common.hpp`. Every input is made in the probe from fixed seeds (the
splitmix64 of `parity/oracle/probe/probe.hpp`) or by hand; no file is read. Each record holds its
inputs and then the engine's results, so a replay needs no generator. The full set holds every
case. The committed set keeps within 512 KB: a kind marked "Committed: all" or "the first n"
commits those, and a kind with a budget of n KB commits, in order, each record that still fits
within n thousand bytes (headers included) and skips one that does not, so its committed records
lean to the smaller cases; a replay takes the count from the file.

Where the engine would leave defined behaviour, the probe does not make the call and says so in
the record (LYCA, LYCD and LYPH), or keeps the input off that path (LYCD and LYCO); each case is
named in its section.

## Values

As in "Values" of `parity/SOP1.md`: `point`, `polygon`, `expolygon`, `surface`, `polyline`,
`entity` (kinds 0 to 8), little-endian numbers, floats by their bits. A list is a `vec`: a `u64`
count and its items. Here also:

| Name | Fields |
| --- | --- |
| `box` | `point` min, `point` max, `bool` defined (a `BoundingBox`) |
| `flow` | `f32` width, `f32` height, `f32` spacing, `f32` nozzle diameter, `bool` bridge |
| `fop` | `f64` value, `bool` percent (a `ConfigOptionFloatOrPercent`) |
| `kv` | `vec` of `str` key and `str` value |
| `try<T>` | `u8` 1 and a `T` when the call returned; `u8` 0 and `str` the exception's `what()` when it threw (`try<>`: nothing after the 1) |

A role is an `ExtrusionRole`, a surface type a `SurfaceType` and a flow role a `FlowRole` (0
external perimeter to 7 support transition), each by its value in the engine's headers. A hash is
the engine's `size_t` as a `u64`. An option's value is its `serialize()` text.

## front-1 (orca-config)

The eleven static classes, in this order: `PrintObjectConfig`, `PrintRegionConfig`,
`MachineEnvelopeConfig`, `GCodeConfig`, `PrintConfig`, `FullPrintConfig`, `SLAPrintConfig`,
`SLAPrintObjectConfig`, `SLAMaterialConfig`, `SLAPrinterConfig`, `SLAFullPrintConfig`. A class of
several parents is used through its first (`PrintConfig` through `MachineEnvelopeConfig`,
`FullPrintConfig` through `PrintRegionConfig`, `SLAFullPrintConfig` through `SLAPrinterConfig`), as
`DynamicPrintConfig::full_print_config()` does; its `keys()` and options are still the whole
class's. Each class writes one LYCK, one LYCA, then its LYCD cases, then its LYCO cases.

### LYCK

front-1. One class's defaults. `str` the class, `u64` `hash()`; `vec` of its keys in `keys()`
order, each `str` key, `u32` the option's `ConfigOptionType`, `str` `opt_serialize(key)`; of the
`DynamicPrintConfig` made from it, `vec<str>` its keys and `bool` it holds as many keys and each
option equals the class's. Committed: all.

### LYCA

front-1. `get_abs_value` of a class's defaults. `str` the class; `vec` of its options of type
float, int, bool, float-or-percent or percent, in key order, each `str` key, `abs` of the key,
then for a float-or-percent `u8` 1, `f64` `get_abs_value(key, 0.4)` and `f64`
`get_abs_value(key, 2.5)`, else `u8` 0. Committed: all.

An `abs` is `try<f64>` of `get_abs_value(key)`, or `u8` 2 when the call is not made. The call
follows the option's `ratio_over` chain (a zero `*_line_width` goes to `line_width`). A missing
option throws, and a float, int, bool or absolute float-or-percent gives its value; but a percent
whose chain reaches an option of any other type (such as `nozzle_diameter`, a vector), a
`coPercent` with a `ratio_over`, or a chain that loops, would dereference a null pointer, read
the option as the wrong type or recurse without end. Those are the 2s.

### LYCD

front-1. A copy of a class's defaults d with options set by `set_deserialize_strict`. `str` the
class, `u32` the case; `vec` of the changes, each `str` key, `str` value, `try<>` the call, `str`
the option's serialization after it, then `u8` 1 and `abs` (LYCA) of the key for an option of
LYCA's types, else `u8` 0. Then `u64` the copy's `hash()`, `bool` copy `==` d, `bool`
`d.equals(copy)`, `vec<str>` `d.diff(copy)`, `u64` the size of `d.equal(copy)`, `vec<str>`
`copy.diff(d)`. A case changes up to 12 distinct keys, each to a value of its type unlike the
current one (an enumeration to another of its `enum_values`). Keys of other types are left out,
and so is every list of enumerations (`coEnums`): setting `z_hop_types` to `Spiral Lift,Auto
Lift` on a `GCodeConfig` crashed the probe. Cases a class: 40, `MachineEnvelopeConfig` 20,
`FullPrintConfig` 60, the SLA classes 4. Committed: the first 4 (2, 6 and 1).

### LYCO

front-1. `apply_only(source, keys, ignore)` on a copy of a class's defaults. `str` the class,
`u32` the case, `bool` ignore; `vec<str>` keys; the source as `vec` of `str` key and `str` value;
`try<>` the call; `vec` of each key of the list, `str` key, `bool` the copy has it, `str` its
value (empty when not); `u64` the copy's `hash()` as the call left it. The list holds 1 to 6
changed keys of the class, a key of the class the source lacks, now and then a key of
`FullPrintConfig` (which the class may lack), `nozzle_diameter#0` and `probe_no_such_key`. The
source holds the changed keys, the `FullPrintConfig` key and `nozzle_diameter` (for the indexed
key), each from `FullPrintConfig`'s defaults or the class's where that lacks it, then the changes
and `nozzle_diameter` of `0.6,0.8`. When the list holds `nozzle_diameter#0` and the class has no
`nozzle_diameter`, the source does not hold it either: `apply_only` would dereference the null
option a static class gives for a key it lacks. Cases and committed counts as LYCD's.

### LYDY

front-1. `u8` 0: `full_print_config()`, `vec<str>` its keys, `bool` it holds as many keys as
`FullPrintConfig` and each option equals its default. `u8` 1: `new_from_defaults_keys` of
`vec<str>` keys, `try` of `vec` of `str` key and `str` value of the result. One record of 0, six
of 1. Committed: all.

### LYAM

front-1. `get_extruder_ams_count` of `vec<str>`: `try` of `vec` of maps, each `vec` of `i32` key
and `i32` count in key order. Ten records. Committed: all.

### LYFO

front-1. `compute_filament_override_value`. `str` key, `str` the old machine option, `str` the new
machine option, `str` the filament option (`ConfigOptionFloats`, `ConfigOptionFloats` and
`ConfigOptionFloatsNullable` made by `deserialize`; `ConfigOptionBools` and
`ConfigOptionBoolsNullable` for `long_retractions_when_cut`), `i32`
`enable_long_retraction_when_cut` (the only option of the full config passed), `vec<i32>` the
filament maps; `try` of `vec<str>` the diff keys and `vec` of `str` key and `str` value of the
overrides config. Fourteen records. Committed: all.

### LYSK

front-1. `has_skirt` of a `DynamicPrintConfig` holding the given options: `kv` the options, then
`try<bool>`. Ten records. Committed: all.

### LYMT

front-1. `MaterialType`. `vec` of `all()`, each `str` name, `i32` min, max, chamber min and
chamber max temperatures, `f64` adhesion coefficient, yield strength and thermal length; then
`vec` of names (each name of the table, then `pla`, `PLA `, the empty name, `Generic`, `PLA+`,
`TPU-95A` and `petg`), each `str` name, `bool` `find` found it, `bool` `get_temperature_range` and
its `i32` min and max, `bool` `get_chamber_temperature_range` and its `i32` min and max, then
`bool` and `f64` of `get_adhesion_coefficient`, `get_yield_strength` and `get_thermal_length`.
Each out value starts at 0 (the `f64`s at -1), so a miss shows what the call writes. One record.

## layer-1 (orca-layer)

### LYFL

layer-1. A flow and what follows from it. `u8` how it is made (0 `Flow(a, b, nozzle)`, 1
`Flow::bridging_flow(a, nozzle)`, 2 `Flow::new_from_config_width(role, width, nozzle, b)`, 3
`Flow()`), `f32` a, `f32` b, `f32` nozzle, `u8` flow role, `fop` width; then `try` of: the
`flow`, `try<f64>` `mm3_per_mm()`, `i64` `scaled_width()`, `scaled_spacing()` and
`scaled_elephant_foot_spacing()`, `f32` w2, h2, s2 and area, `f64` ratio, `try<flow>` of
`with_width(w2)`, `with_height(h2)`, `with_spacing(s2)`, `with_cross_section(area)` and
`with_flow_ratio(ratio)`, the reference `flow` (`Flow(0.45, 0.2, 0.4)`), `bool` `==` and `!=` it,
`try<bool>` `<` it. The area is the flow's own cross section, 5e-5 above it (within the engine's
EPSILON), above it or below it, and 0.1 when `mm3_per_mm` threw. 17 hand-made cases, then 1500
random. Committed: the first 57.

### LYFS

layer-1. `f32` w, h and nozzle; `try<f32>` `rounded_rectangle_extrusion_spacing(w, h)`, `f32`
`rounded_rectangle_extrusion_width_from_spacing(w, h)`, `f32` `bridge_extrusion_spacing(w)`, `f32`
`auto_extrusion_width(role, nozzle)` of the eight flow roles. 400 cases (the first with w 0, the
second with h 0). Committed: the first 40.

### LYFW

layer-1. `Flow::extrusion_width(key, config, first)`. `kv` the configuration's options
(`nozzle_diameter`, absent now and then, and the eleven line widths `line_width`,
`initial_layer_line_width`, `inner_wall_line_width`, `outer_wall_line_width`,
`sparse_infill_line_width`, `internal_solid_infill_line_width`, `bridge_line_width`,
`top_surface_line_width`, `support_line_width`, `skin_infill_line_width` and
`skeleton_infill_line_width`, of which `line_width` and `internal_solid_infill_line_width` are
absent now and then), `u32` first printing extruder; `vec` of each line width the configuration
holds, `str` key and `try<f64>`. 300 cases. Committed: a budget of 10 KB.

### LYRO

layer-1. The roles. `vec` of each `ExtrusionRole` from 0 to `erCount`, `u8` role, `str`
`role_to_string`, `u8` `string_to_role` of that name, `u8` bits: 1 `is_perimeter`, 2
`is_internal_perimeter`, 4 `is_external_perimeter`, 8 `is_infill`, 16 `is_top_surface`, 32
`is_solid_infill`, 64 `is_bridge`; then `vec` of other texts, `str` and `u8` `string_to_role`. One
record.

### LYEN

layer-1. A path (`ExtrusionPath`, or now and then `ExtrusionPathContoured`): the `entity`, `f32`
eps, `f64` d, `f64` tol, `vec<expolygon>` clip; `f64` `length()`, `point` `first_point()` and
`last_point()`, `bool` `is_closed()`, `f64` `min_mm3_per_mm()` and `total_volume()`, `polyline`
`as_polyline()`, `vec<polyline>` `collect_polylines`, `vec<point>` `collect_points`; the covers:
`try<vec<polygon>>` of `polygons_covered_by_width(0)`, `(eps)`, `polygons_covered_by_spacing(0)`
and `(eps)`. Then the `entity` of a clone after `reverse()`, after `clip_end(d)`, after
`simplify(tol)` and after `ExtrusionPath::simplify_by_fitting_arc(tol)`; the collection `entity`s
`intersect_expolygons(clip)` and `subtract_expolygons(clip)` fill. A clone's `inset_idx` is -1: the
path's copy constructor does not copy it. 2 hand-made cases (a 1 mm wall, a path of one repeated
point), then 598 random. Committed: a budget of 25 KB.

### LYEM

layer-1. An `ExtrusionMultiPath` of joined paths: the `entity`, `f32` eps; `f64` `length()`, `u8`
`role()`, `point` first and last point, `f64` `min_mm3_per_mm()` and `total_volume()`, `polyline`
`as_polyline()`, `vec<polyline>` `collect_polylines`, `vec<point>` `collect_points`, the four
covers as LYEN's; the `entity` of a copy after `reverse()`, then `bool` its `can_reverse()` after
`set_reverse()`. 300 cases. Committed: a budget of 12 KB.

### LYEL

layer-1. An `ExtrusionLoop` (a star cut into one to four paths of wall roles): the `entity`,
`point` v (a vertex of its polygon, one within 600 of it or one far from it), `point` q, `bool`
prefer, `f64` eps, `f64` d, `f32` cover eps; `polygon` `polygon()`, `f64` `length()`, `u8`
`role()`; of a copy `bool` `is_clockwise()`, `bool` `is_counter_clockwise()`, `bool`
`make_clockwise()` and the `entity`; of a copy `bool` `make_counter_clockwise()` and the `entity`;
of a copy `bool` `split_at_vertex(v)` and the `entity`; `get_closest_path_and_point(q, prefer)` as
`u64` path, `u64` segment and `point` foot; the `entity` of a copy after `split_at(q, prefer,
eps)`; `vec<entity>` `clip_end(d)`; `bool` `has_overhang_point` of the vertex itself; `bool`
`is_smooth()` and `is_smooth(0.5, 0.1)`; `polyline` `as_polyline()`; `f64` `min_mm3_per_mm()` and
`total_volume()`; the four covers. 400 cases. Committed: a budget of 25 KB.

### LYES

layer-1. An `ExtrusionLoopSloped` made from a loop's paths: `vec<entity>` the paths, `f64` seam
gap, `f64` slope min length, `f64` slope max segment length, `f64` start slope ratio, `u8` loop
role, `f64` d; then `try` of: the `entity`, `u64` `get_all_paths().size()`, `f64`
`slope_path_length()`, `vec` of each start's `interpolate(0.5)` (`f64` z ratio, `f64` e ratio)
and `bool` `is_flat()`, then the `entity` of a copy after `clip_slope(d)`, after `clip_end(d)` and
after `clip_front(d)`. 200 cases. Committed: a budget of 10 KB.

### LYEC

layer-1. An `ExtrusionEntityCollection` of one to six entities (paths, oriented and contoured
paths, multi paths, loops and collections two deep, none empty): the `entity`, `f32` eps, `point`
start, `u8` role r, `u64` index i; `u8` `role()`, `u64` `items_count()`, `bool`
`has_perimeters()`, `has_infill()`, `has_solid_infill()`, `can_sort()` and `can_reverse()`, `f64`
`min_mm3_per_mm()` and `total_volume()`, `point` first and last point, `vec<polyline>`
`collect_polylines`, `vec<point>` `collect_points`, the four covers; the `entity` of
`flatten(false)` and of `flatten(true)`, of a copy after `reverse()`; `vec<entity>`
`ExtrusionPaths(collection)`; the `entity` of `chained_path_from(start, erMixed)` and of
`chained_path_from(start, r)`; `vec<u64>` the indices `filter_by_extrusion_role(entities, r)`
keeps; the `entity` of a copy after `replace(i, path)` (a gap fill path, whose fields the record
shows), of a copy after `remove(i)`; `bool` `can_reverse()` and `bool` `no_sort` of a collection
assigned from it (the assignment does not copy the reverse flag); `bool` `can_reverse()` of a copy
after `set_reverse()`. 300 cases. Committed: a budget of 30 KB.

### LYEA

layer-1. The append helpers of `ExtrusionEntity.hpp`: `vec<polyline>` the input (zero to six,
some of one point, some closed, some starting near the last one's end), `u8` role, `f64` mm3 per
mm, `f32` width, `f32` height, `bool` can reverse; `vec<entity>` of `extrusion_paths_append`,
`extrusion_entities_append_paths` (with can reverse), `extrusion_entities_append_paths_with_wipe`,
`extrusion_entities_append_loops` (of each polyline's points as a polygon) and
`extrusion_entities_append_loops_and_paths`. 200 cases. Committed: a budget of 8 KB.

### LYVW

layer-1. `vec` of one to three thick polylines, each `vec<point>`, `vec<f64>` widths (two a
segment), `bool` and `bool` the endpoint flags; `u8` role, `flow`, `f32` tolerance, `f32` merge
tolerance; `try<vec<entity>>` `variable_width(polylines, role, flow)`; `vec` of each polyline's
`try<entity>` `thick_polyline_to_multi_path(polyline, role, flow, tolerance, merge tolerance)`.
400 cases. Committed: a budget of 15 KB.

## layer-2 (orca-layer)

### LYSU

layer-2. A list of surfaces: `vec<surface>`; for each surface `u16` bits (1 `empty`, 2 `is_top`,
4 `is_bottom`, 8 `is_bridge`, 16 `is_internal_bridge`, 32 `is_external`, 64 `is_internal`, 128
`is_solid`, 256 `is_solid_infill`), `f64` `area()`, `box` `get_extents`, `vec<polygon>`
`to_polygons`, `str` `surface_type_to_color_name`; then of the list `box` `get_extents`,
`vec<polygon>` `to_polygons`, `vec<expolygon>` `to_expolygons`, `u64` `number_polygons`, and
`bool` `surfaces_could_merge(a, b)` of every ordered pair, a by a. 300 lists (the first empty, the
second ending in an empty `stInternalVoid` surface), then one record of three `str`: the colour
names of `SurfaceType(-1)`, `stCount` and `stInternalAfterExternalBridge`. Committed: a budget of
30 KB over the 300, and the last.

### LYSC

layer-2. `SurfaceCollection`'s operations, each on a collection of the list as given:
`vec<surface>`, `u32` type t, `u32` type t2, `f64` tol, `point` q, `point` q2 (the first contour
point of one of the surfaces); the surfaces after `simplify(tol)`; `group()` as `vec` of groups,
each `vec<u64>` surface indices; `vec<u64>` the indices of `filter_by_type(t)` and of
`filter_by_types({t, t2})`; `vec<polygon>` `filter_by_type(t, &polygons)`; `u16` `has` of each
type as bit 1 << type; `bool` `any_internal_contains` of q and of q2, `bool`
`any_bottom_contains` of q and of q2; the surfaces after `keep_type(t)`, after `keep_types({t,
t2})`, after `remove_type(t)`; after `remove_type(t, &expolygons)` and `vec<expolygon>` those;
after `remove_types({t, t2})`; after `change_to_new_type(t, t2)`, then after `set_type(t)`. 300
cases. Committed: a budget of 30 KB.

## layer-3 (orca-layer)

The ten prints, in order: defaults, widths, percent widths on a 0.6 nozzle, automatic widths, brim
and skirt, names and scales, nothing printable, two filaments, supports and a raft. The last three
are outside domain A; their records show where a replay stops with NotPorted. Each print writes
its LYPC, then an LYPR for each printing region of each object, an LYSF for each object, one LYPH,
one LYPT, then its LYLA records.

### LYPC

layer-3. A print the probe builds: `DynamicPrintConfig::full_print_config()` with the case's
options set by `set_deserialize_strict`, a `Model` of generated meshes, each instance offset,
scaled and marked printable, each object's options set and `ensure_on_bed()` called, then
`Print::apply`. `u32` case, `str` label, `bool` in domain A, `kv` the options; `vec` of objects,
each `str` name, `str` input file, `u8` mesh (0 `make_cube(x, y, z)`, 1 `make_cylinder(x, z)`),
`f64` x, y and z, `vec` of instances (`f64` offset x and y, `f64` scale x, y and z, `bool`
printable), `kv` the object's options; `try<u32>` the `ApplyStatus`; `vec` of the print objects,
each `vec<point>` its instances' shifts, `i64` size x, y and z, `u64` its config's `hash()`,
`vec<u64>` its printing regions' `config_hash()`, `vec<u32>` `object_extruders()`;
`chain_print_object_instances(print)` as `vec` of `u64` object and `u64` instance; `u64` the print
config's `hash()`, `bool` `has_brim()`. Committed: all.

### LYPR

layer-3. A printing region of an object: `u32` case, `u64` object, `u64` region, `u64`
`config_hash()`; `try<u32>` `extruder(role)` of the eight flow roles; `try<flow>` `flow(object,
role, h, first layer)` by role, then h (the object's layer height, 0.1, 0), then first layer
(false, true); `f64` `nozzle_dmr_avg` and `f64` `bridging_height_avg` of the print config;
`vec<u32>` `collect_object_printing_extruders(print)`; `vec<u32>` the static
`collect_object_printing_extruders(print config, region config, brim)` without and with a brim.
Committed: all.

### LYSF

layer-3. `u32` case, `u64` object; `try<flow>` of `support_material_flow(object)`, `(object,
0.15)`, `support_transition_flow(object)`, `support_material_1st_layer_flow(object)`, `(object,
0.25)`, `support_material_interface_flow(object)` and `(object, 0.15)`. Committed: all.

### LYPH

layer-3. PrintBase's placeholders as G-code export reaches them: `u32` case; `kv` an empty
`DynamicConfig` after `update_object_placeholders(config, ".gcode")`, in key order; then `vec` of
`output_filename(format, ".gcode", base)` for the formats (empty,
`{input_filename_base}_{num_objects}`, `[input_filename_base].gcode`, `{first_object_name}`,
`{scale[0]}`, `{bad`) by the bases (empty, `base.part`), each `str` format, `str` base and
`try<str>`, or `u8` 2 for the empty format with no base when no instance is printable: the engine
reads `input_filename_base` unchecked there, so the probe does not make that call. Committed: all.

### LYPT

layer-3. `u32` case; `vec<i32>` `get_filament_maps()`, `u32` `get_filament_map_mode()`,
`vec<f64>` `get_extruder_printable_height()`, `vec` of `vec<polygon>`
`get_extruder_printable_polygons()`, the same of `get_extruder_unprintable_polygons()`, `u64`
`get_extruder_id(f)` for f from 0 to 3. Committed: all.

### LYLA

layer-3. A layer added to the case's first object by `PrintObject::add_layer`, with one to three
regions of its own: each a `PrintRegion` of the object's first printing region config with some
options set, added by `add_region`, and given slices and fill boundaries. `u32` case, `u64` id,
`f64` height, print z and slice z, `u64` regions; each region `kv` its options, `u64` its config's
`hash()`, `vec<surface>` slices, `vec<expolygon>` fill boundaries. Then: each region's fill
surfaces after `slices_to_fill_surfaces_clipped()`; `bool` `empty()`; `vec<expolygon>` lslices
after `make_slices()`; `vec<expolygon>` `merged` of `scale_(0)`, `scale_(0.1)` and `scale_(1)`;
each region's `box` `get_extents`, the regions' `box`; for each region and each of the eight flow
roles `try<flow>` of `flow(role)`, `flow(role, 0.3)`, `bridging_flow(role, false)` and
`bridging_flow(role, true)`; `try<f64>` `get_sparse_infill_max_void_area()`; each region's
`vec<expolygon>` raw slices after `backup_untyped_slices()`; each region's `vec<surface>` slices
after `restore_untyped_slices()`, then after `restore_untyped_slices_no_extra_perimeters()`; `u64`
`get_extruder_id(0)` and `get_extruder_id(1)`. 40 layers on each of the first four prints, 10 on
the other prints of domain A with an object and on two filaments. Committed: a budget of 40 KB
over the first 3 layers of each of the first four prints and the first layer of the others.

### LYCH

layer-3. The chaining of `ShortestPath.cpp`: `u8` kind, `bool` with a start point, `point` start.
Kind 0: `vec<entity>` entities, `chain_extrusion_entities` as `vec` of `u64` index and `bool`
reversed, `vec<entity>` after `reorder_extrusion_entities` by it. Kind 1: `vec<entity>` paths,
`vec<entity>` after `chain_and_reorder_extrusion_paths`. Kind 2: a collection `entity` holding,
among others, entities without end points (an empty path, multi path, loop or collection), then
the `entity` of `chained_path_from(start, erMixed)`. `reorder_extrusion_paths` is not called: its
declaration and definition differ. 200 cases. Committed: a budget of 20 KB.
