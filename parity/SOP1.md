# SOP1: the oracle's dump files

SOP1 is the container the oracle writes with `SKETCH_ORCA_ORACLE_DUMP=<folder>` (oracle patch
0005, `src/libslic3r/OracleDump.cpp` in the oracle's checkout). Each file is one step boundary: the
state of the print, of one object or of the front end at that point, never in the middle of a
step. This page is the format, version 1, field by field, in the order the oracle writes it. The
harness reads it in `scripts/lib/parity/neutral.ts` (`readSop1`, `parseHead`, `parseRun`).

## Files

The oracle writes one file per boundary into the dump folder, named after the boundary and the
number of times this run has reached it (counting from 1):

| File | Written | Records |
| --- | --- | --- |
| `front-objects-<n>.sop1` | in the front end, once the objects are placed (after arranging and the build volume check) | HEAD, RUN, then for each model object: MODL, a CONF `model-object`, and a CONF `model-volume` for each of its volumes; END |
| `front-config-<n>.sop1` | in the front end, once the config is resolved and applied to the print, before each slice | HEAD, RUN, CONF `job`, CONF `print-full`, CONF `print`, then for each print object a CONF `object` and a CONF `region` for each of its printing regions; END |
| `object<i>-<step>-<n>.sop1` | after the print object `i` (its index in the print) finishes the step `<step>` (`PrintObjectBaseWithState::set_done`) | HEAD, RUN, OBJ, a LAYR for each layer, a SUPL for each support layer; END |
| `print-<step>-<n>.sop1` | after the print finishes the step `<step>` (`PrintBaseWithState::set_done`) | HEAD, RUN, PRNT, TOOL, WIPE, STAT, CNFL; END |

The object steps are `posSlice`, `posPerimeters`, `posEstimateCurledExtrusions`,
`posPrepareInfill`, `posInfill`, `posIroning`, `posContouring`, `posSupportMaterial`,
`posSimplifyPath`, `posSimplifySupportPath`, `posDetectOverhangsForLift`, `posSimplifyWall` and
`posSimplifyInfill`. The print steps are `psWipeTower`, `psSkirtBrim`, `psGCodeExport` and
`psConflictCheck`. A step the engine skips writes nothing. Dumps are written one at a time, each
whole, under one lock.

## The container

```
magic     4 bytes  "SOP1"
version   u32      1
records   ...      each: kind (4 bytes), length (u64), payload (length bytes)
END       record   kind "END ", payload one u64: the number of records before it
```

A kind is four ASCII letters, padded with spaces: `HEAD`, `RUN `, `CONF`, `MODL`, `OBJ `, `LAYR`,
`SUPL`, `PRNT`, `TOOL`, `WIPE`, `STAT`, `CNFL`, `END `. Nothing follows END. A reader refuses a file
whose magic, version, lengths or END count do not hold.

## Values

Every value is little-endian.

| Name | Bytes |
| --- | --- |
| `u8`, `u16`, `u32`, `u64` | unsigned integers of 1, 2, 4 and 8 bytes |
| `i32`, `i64` | two's complement integers of 4 and 8 bytes |
| `bool` | a `u8`, 0 or 1 |
| `f32`, `f64` | the exact IEEE 754 bit pattern as a `u32` or `u64`: no rounding, no text, so -0, NaN payloads and denormals survive |
| `str` | a `u32` byte count, then the bytes (UTF-8 as the engine holds them), no terminator |
| `vec<T>` | a `u64` count, then that many `T` |

Compound values:

| Name | Fields |
| --- | --- |
| `point` | `i64` x, `i64` y (scaled coordinates, as `Point`) |
| `polygon` | `vec<point>` |
| `expolygon` | `polygon` contour, `vec<polygon>` holes |
| `surface` | `u32` surface type (`SurfaceType`), `expolygon`, `f64` thickness, `u16` thickness layers, `f64` bridge angle, `u16` extra perimeters |
| `fitting` | `u64` start point index, `u64` end point index, `u8` path type (`EMovePathType`), `bool` is arc, `point` centre, `f64` radius, `f64` length, `f64` angle in radians, `f64` polar start theta, `f64` polar end theta, `point` start, `point` end, `u8` direction (`ArcDirection`) |
| `polyline` | `vec<point>`, `vec<fitting>` |
| `matrix` | 16 `f64`, column by column (as Eigen stores a `Transform3d`) |
| `entity` | an extrusion entity: a `u8` kind, then its fields (below) |

An `entity`'s kind is found from its exact type:

| Kind | Type | Fields after the kind |
| --- | --- | --- |
| 0 | any other | `str` the type's name |
| 1 | `ExtrusionPath` | the path fields |
| 2 | `ExtrusionPathContoured` | the path fields, `vec<f64>` z diffs |
| 3 | `ExtrusionPathSloped` | the path fields, `f64` begin z ratio, `f64` begin e ratio, `f64` end z ratio, `f64` end e ratio |
| 4 | `ExtrusionPathOriented` | the path fields |
| 5 | `ExtrusionMultiPath` | `i32` inset index, `bool` can reverse, `vec<entity>` paths |
| 6 | `ExtrusionLoop` | the loop fields |
| 7 | `ExtrusionLoopSloped` | the loop fields, `vec<entity>` starts, `vec<entity>` ends |
| 8 | `ExtrusionEntityCollection` | `i32` inset index, `bool` no sort, `bool` can reverse, `vec<entity>` entities |

The path fields: `u8` role (`ExtrusionRole`), `i32` inset index, `vec` of points as `i64` x, y, z,
`vec<fitting>`, `f64` overhang degree, `i32` curve degree, `f64` mm3 per mm, `f32` width, `f32`
height, `f64` smooth speed, `bool` z contoured, `bool` forced no extrusion, `bool` can reverse.

The loop fields: `i32` inset index, `u8` loop role (`ExtrusionLoopRole`), `vec<entity>` paths.

Enumerations are written as their values in OrcaSlicer 2.4.2's headers, named in brackets above.

## Records

### HEAD

What the file is of: `str` source (`front`, `object` or `print`), `str` name (the step, or
`objects` or `config`), `i64` object (its index in the print for `object`, else -1), `u32` the
boundary's count in this run (the `<n>` of the file name).

### RUN

The knobs and what the oracle's run reports at this boundary:

- `vec` of knobs, for `SKETCH_ORCA_ORACLE_TIME`, `_SEED`, `_THREADS` and `_FMA3` in that order:
  `str` name, `bool` set, `str` value (empty when not set). The dump knob is not listed: its value
  is the folder, which differs from run to run;
- `i32` `tbb::this_task_arena::max_concurrency()`: with `SKETCH_ORCA_ORACLE_THREADS=P`, P when the
  arena of oracle patch 0003 reports it;
- `u64` `tbb::global_control::active_value(max_allowed_parallelism)`: 1 under that knob;
- `i32` `_get_FMA3_enable()` (x64 MSVC builds), or -1.

### CONF

A config: `str` scope, `i64` object index (or -1), `i64` part index (or -1), then a `vec` of
options sorted by key, each: `str` key, `u32` option type (`ConfigOptionType`), `str` the value as
the engine serializes it, `vec<f64>` the value's floats as bits. The floats are those of a float,
floats, float-or-percent list, 2D point, 2D points or 3D point option; for any other type the
vector is empty. An option the config lists but cannot give is written with type 0, an empty value
and no floats.

| Scope | Config | Object | Part |
| --- | --- | --- | --- |
| `model-object` | the model object's own settings | model object | -1 |
| `model-volume` | a volume's own settings | model object | volume |
| `job` | the config the front end resolved and applied | -1 | -1 |
| `print-full` | `Print::full_print_config()` | -1 | -1 |
| `print` | `Print::config()` | -1 | -1 |
| `object` | `PrintObject::config()` | print object | -1 |
| `region` | a printing region's config | print object | region |

### MODL

A model object, as placed: `i64` index, `str` name, `bool` printable, `vec<f64>` layer height
profile, `vec` of instances (`matrix`, `bool` printable), `vec` of volumes, each: `str` name, `i32`
type (`ModelVolumeType`), `matrix`, `vec` of vertices as three `f32`, `vec` of triangles as three
`i32` vertex indices.

### OBJ

A print object: `i64` index, `str` name (its model object's), size as three `i64`, `point` centre
offset, `matrix` transformation, `f64` max z, `vec` of instances (`point` shift, `u64` id), `u64`
number of layers, `u64` number of support layers.

### LAYR

One layer of the object, in order: `u64` position, `u64` id, `f64` slice z, `f64` print z, `f64`
height, `bool` slicing errors, `vec<expolygon>` lslices, lslices extrudable, overhangs, sharp
tails and cantilevers (five lists in that order), `vec<f32>` sharp tail heights, then a `vec` of
regions, each: `i32` print region id, `vec<surface>` slices, `vec<expolygon>` raw slices,
`vec<surface>` fill surfaces, `vec<expolygon>` fill expolygons, `vec<expolygon>` fill
expolygons without overlap, `vec<polyline>` unsupported bridge edges, and three `entity`s: thin
fills, perimeters, fills.

### SUPL

One support layer, in order: `u64` position, `u64` id, `f64` slice z, `f64` print z, `f64` height,
`u32` support type, `u64` interface id, `vec<expolygon>` lslices, support islands and base areas,
`entity` support fills.

### PRNT

The print's skirt and brims: `entity` skirt; a `vec` of skirt and brim groups, each: `entity`
skirt, `vec<i64>` object indices, a `vec` of brims (`entity` brim, `vec<i64>` object indices);
`bool` has a shared per-object skirt; the brim map and then the support brim map, each a `vec` of
(`i64` object index, `entity`) sorted by object index; `polygon` first layer convex hull; `vec<point>`
skirt convex hull; `vec<u32>` the filaments used, then `vec<u32>` those used on the first layer;
`bool` support is used. Objects are written as their index in the print, never as an `ObjectID`,
whose values depend on what was made before.

### TOOL

The tool ordering: `u32` first extruder, `u32` last extruder, `vec<u32>` all extruders, then a
`vec` of layers, each: `f64` print z, `bool` has object, `bool` has support, `vec<u32>`
extruders, `u32` extruder override, `bool` has skirt, `bool` has wipe tower, `u64` wipe tower
partitions, `f64` wipe tower layer height, `bool` has custom G-code.

### WIPE

The wipe tower data: `i32` number of tool changes, `f32` depth, `f32` brim width, `vec<f32>` used
filament, a `vec` of (`f32` z, `f32` depth), `bool` bounding box defined, `f64` min x, min y, max x,
max y, `f32` rib offset x and y, `bool` has priming and, when it has, a `vec` of tool changes, a
`vec` of layers each a `vec` of tool changes, `bool` has final purge and, when it has, one tool
change.

A tool change: `f32` print z, `f32` layer height, `str` G-code, a `vec` of extrusions (`f32` x,
`f32` y, `f32` width, `u32` tool), `f32` start x and y, `f32` end x and y, `f32` elapsed time,
`bool` priming, `bool` is tool change, `bool` is contact, `f32` tool change start x and y,
`vec` of wipe path points (`f32` x, `f32` y), `f32` purge volume, `i32` initial tool, `i32` new
tool, `bool` is finish first.

`WipeTowerData::height` is not written: OrcaSlicer 2.4.2 sets it on one of its two wipe tower
paths only, so on the other it holds whatever the memory held.

### STAT

The print statistics: `str` estimated normal print time, `str` estimated silent print time,
`f64` total used filament, `f64` total extruded volume, `f64` total cost, `i32` total tool changes,
`f64` total weight, `f64` total wipe tower cost, `f64` total wipe tower filament, `u32` initial
tool, a `vec` of (`u64` filament, `f64` used).

### CNFL

The conflict check: `bool` has a conflict and, when it has, `str` first object's name, `str`
second object's name, `f64` height, `i32` layer.

### END

`u64`: the number of records before it.

## Versions

A change to any record, its fields or their order is a new version. The reader refuses a version it
does not know.
