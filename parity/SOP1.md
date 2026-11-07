# SOP1: the oracle's dump files

SOP1 is the container the oracle writes with `SKETCH_ORCA_ORACLE_DUMP=<folder>` (oracle patch
0005, `src/libslic3r/OracleDump.cpp` in the oracle's checkout). Each file is one step boundary: the
state of the print, of one object or of the front end at that point, never in the middle of a
step. This page is the format, version 1, field by field, in the order the oracle writes it. The
harness reads it in `scripts/lib/parity/neutral.ts` (`readSop1`, `parseHead`, `parseRun`). The
probe program writes the same container with records of its own ("Probe files", below).

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

## Probe files

The probe programs (`parity/oracle/probe/`, built and run by `bun scripts/orca.ts parity probe`,
`scripts/lib/parity/probe.ts`) write SOP1 files too, in the same container and values, with
record kinds of their own. They record what the C++ engine's foundations and libraries do, as the
engine is compiled and linked, for the Rust crates' probe tests. Each probe of the registry
(`PROBE_REGISTRY`, from 2026-11-07) is a program of its own, `sketch-orca-probe-<name>`, that
writes one file; `parity probe --only <name>` builds and runs one and writes only its file. Each
run writes two sets from the same cases: the full set into `<work>/parity/probes/` and the
committed subset into `parity/probes/` (the first cases of each record, or the records of fewer
sizes, thread counts and formats). The committed files keep to their budgets: the five
foundation files under 8 MB together, every other file 512 KB at most. A file is HEAD, PRUN, the
records below, END.

| File | Program's task | Records |
| --- | --- | --- |
| `crt.sop1` | O1 | MATH, MATS, CAST, RAND |
| `fmt.sop1` | O1 | PRTF, TOCH, OSTR, TOST, PARS |
| `stl.sop1` | O1 | SORT, VALS, HASH, UMAP, USET, RNGS, SSEQ, SHUF |
| `tbb.sop1` | O1 | TBBA, TBBF, TBBR, TBBO |
| `configdef.json` | O1 | not SOP1: `print_config_def` with every field of each option, the preset sections' keys, and `handle_legacy` on the legacy and defined keys (`configdef.cpp` says what it holds) |
| `geom.sop1` | o2-geom | none yet: a stub (`orca-geom`, `orca-eigen`) |
| `qhull.sop1` | o2-mesh | none yet: a stub (`orca-qhull`) |
| `mesh.sop1` | o2-mesh | none yet: a stub (`orca-mesh`) |
| `cstdio.sop1` | o2-mesh | none yet: a stub (`orca-fmt`'s `stdio` and `scanf`) |
| `zip.sop1` | o2-model | none yet: a stub (`orca-zip`) |
| `xml.sop1` | o2-model | none yet: a stub (`orca-xml`) |
| `model.sop1` | o2-model | none yet: a stub (`orca-model`) |
| `arrange.sop1` | o2-arrange | none yet: a stub (`orca-arrange`, `orca-exact`) |
| `placeholder.sop1` | o2-front | none yet: a stub (`orca-placeholder`) |

A stub is HEAD, PRUN and END: the task that fills it adds its records here, and the crate's probe
test fails as soon as the file holds a record it does not replay.

Inputs come from splitmix64 generators with fixed seeds (`probe.hpp`), so two runs write the same
bytes (`parity probe --check`). An argument or a result that is a `double` or `float` is its bit
pattern as a `u64`, a `float`'s zero-extended; an integer result is sign-extended to a `u64`.

### HEAD in a probe file

`str` `probe`, `str` the probe's name (`crt`, `fmt`, `stl`, `tbb`, `geom` and the rest of the
table), `i64` -1, `u32` 1.

### PRUN

`u32` `_MSC_FULL_VER`, `u32` `_MSVC_STL_UPDATE`, `i32` the UCRT's FMA3 choice at start
(`_get_FMA3_enable`), `str` the set (`committed` or `full`).

### MATH

One UCRT maths function in one FMA3 state: `str` name (as the oracle's linker map lists it),
`i32` FMA3 state, `str` signature, `u64` cases, then for each case its arguments and then its
results, each a `u64`. The signatures: `d>d`, `f>f`, `dd>d`, `ff>f`, `ddd>d` (doubles and floats
in, one out); `d>l` (`long long`), `d>i` (`long` or `int`); `d>di` (`frexp`: the mantissa, then the
exponent); `di>d` (`ldexp`: the double, then the `int`); `dd>dd` (`__vdecl_floor2`: two lanes in,
two out). `nexttoward`'s second argument is passed as a `long double`, which is a `double` in MSVC.
Each function is recorded with FMA3 on (1) and off (0): special values first, then seeded ones.

### MATS

`nan(const char*)`: `str` `nan`, `i32` FMA3 state, `u64` cases, then for each `str` the argument
and `u64` the result's bits.

### CAST

MSVC 14.42's conversion of a `double` or `float` to an integer type, through a function the
compiler cannot inline: `str` `f64>i8` (`f64` or `f32`, then `i8`, `u8`, `i16`, `u16`, `i32`,
`u32`, `i64`, `u64`), `u64` cases, then for each `u64` the argument's bits and `u64` the result.

### RAND

`rand()`: `str` what (`fresh-thread`: a new thread's first values; `srand`: a new thread's values
after its `srand(seed)`; `per-thread`: a thread's first value, then its values after another
thread called `srand(99)`), `u32` seed, `vec<i32>` the values.

### PRTF

`snprintf` with one argument: `str` the format, `str` its argument's type (`i32`, `u32`, `i64`,
`u64`, `f64`), `u64` cases, then for each `u64` the argument (an integer's value, a `double`'s
bits), `str` the output, `i32` the return value.

### TOCH

`std::to_chars` of a `double` (width 64) or `float` (32): `str` kind (`shortest`, `format`,
`precision`, `short-buffer`), `u8` width, `u64` cases, then for each `u64` bits, `i32` format (-1
none, 0 scientific, 1 fixed, 2 hex, 3 general), `bool` has precision, `i32` precision, `u32` the
buffer's size, `i32` the `errc` (0 for none), `str` the text written.

### OSTR

`std::ostringstream << double`: `u64` cases, then for each `u64` bits, `u8` floatfield (0 none,
1 fixed, 2 scientific, 3 both), `u8` flags (1 showpos, 2 showpoint, 4 uppercase, 8 the stream
went bad), `u8` adjustfield (0 none, 1 left, 2 internal, 3 right), `i64` precision, `i64` width,
`u8` fill, `u8` locale (0 classic; 1 and 2 a `numpunct` with decimal point `,` and separator `.`,
grouping `"\3"` and `"\1\2"`), `str` the text.

### TOST

`std::to_string(double)`: `u64` cases, then for each `u64` bits and `str` the text.

### PARS

A parser: `str` function (`strtod`, `strtof`, `stod`, `stof`, or `from_chars-f64-` or
`from_chars-f32-` followed by `scientific`, `fixed`, `hex` or `general`), `u64` cases, then for
each `str` the input, `u64` the result's bits (`0xDEADBEEF` where the function left it), `i64`
the characters consumed, `i32` status (`errno` for the C functions, 0, 1 invalid argument or 2 out
of range for `sto*`, the `errc` for `from_chars`).

### SORT

One run of an algorithm with a comparator that logs its calls: `str` algorithm (`sort`,
`stable_sort`, `stable_sort_cap` with a given temporary buffer, `nth_element`, `partial_sort`,
`inplace_merge`, `heap`, `priority_queue`, `bounds`, `unique`), `u8` key type (0 `int`, 1
`double`), `vec<i64>` parameters (`stable_sort_cap`: the buffer's size; `nth_element`: nth;
`partial_sort` and `inplace_merge`: the middle; `bounds`: the keys searched for), `vec<u64>` the
input's keys (an `int` sign-extended, a `double`'s bits), `vec<i32>` the input's ids, `vec<i32>`
every comparator call as the two ids it compared (the searched key's id is -1), `vec<i32>` the
result's ids, `vec<i64>` what else it returned (`heap`: `is_heap_until`'s index; `bounds`: each
key's `lower_bound` and `upper_bound`; `unique`: the new size). `heap` runs `is_heap_until`,
`make_heap`, `pop_heap` down to one, `push_heap` up to all and `sort_heap` under one log; its
result is the ids after `make_heap`, after the pops and at the end. `priority_queue`'s result is
the order it popped.

### VALS

`str` name, `vec<u64>` inputs, `vec<u64>` outputs: `max-min-clamp-f64` (`std::max`, `std::min`
and `std::clamp(2.0, lo, hi)` on two doubles), `max-min-f32`, `elements-f64` (`max_element`,
`min_element` on a pointer range and on a vector's iterators, and `minmax_element`'s two, as
indices), and the initializer-list forms.

### HASH

`std::hash`: `str` type (`i32`, `i64`, `u64`, `f64`, `f32`, `string`), `u64` cases, then for each
`str` the value's bytes and `u64` the hash.

### UMAP

`std::unordered_map<int, int>` through a script: `str` name, `u64` operations, then for each `u8`
op, `i64` a, `i64` b; then `u64` snapshots, then for each `vec<i64>` the key and value pairs in
iteration order, `u64` bucket count, `u64` size. The ops: 0 `m[a] += b`, 1 `insert({a, b})`, 2
`emplace(a, b)`, 3 `erase(a)`, 4 `reserve(a)`, 5 `rehash(a)`, 6 `clear()`, 7 `m[a] = b`, 8 a copy
assigned back, 9 assignment to a map that held `{1: 2}` and moved back, 10
`max_load_factor(a / 1000)`, 11 `m = Map(a)` (a buckets), 12 a snapshot.

### USET

`std::unordered_set` of `string`, `f64` or `u64`: `str` type, `u64` operations, then for each
`u8` op (0 insert, 1 erase) and `str` the key's bytes; then `u64` count and each key's bytes in
iteration order, `u64` bucket count, `u64` size.

### RNGS

`str` engine (`mt19937`, `mt19937_64`, or either with `-seed_seq`), `vec<u64>` seed (empty for
the default seed; for a `seed_seq`, the word count and then the words), `u64` draws, then for each
`u8` kind, `u64` a, `u64` b, `u64` result. The kinds: 0 the engine's next value, 1
`discard(a)`, 2 `uniform_int_distribution<int>(a, b)`, 3 `<long long>`, 4 `<unsigned long
long>`, 5 `<size_t>`, 6 `<short>`, 7 `<unsigned>`, 8 `uniform_real_distribution<double>` (a and b
bits), 9 `<float>`, 10 `generate_canonical<double, 53>`, 11 `generate_canonical<float, 24>`, 12
`uniform_real_distribution<double>()`.

### SSEQ

`std::seed_seq::generate`: `vec<u32>` the seed words, `vec<u32>` ten words generated.

### SHUF

`std::shuffle` of 0, 1, ... n-1: `str` engine, `u64` seed, `u32` n, `vec<i32>` the result, `u64`
the engine's next value after it.

### TBBA

`this_task_arena::max_concurrency()`: `str` where (`outside`, under the global control, or
`arena`), `i32` the arena's concurrency P (0 outside), `i32` the value.

### TBBF

The leaves of a `parallel_for` in the order they ran: `str` range (`long long`, `size_t`, `int`:
`blocked_range` of that type; `2d`: `blocked_range2d<int>`), `str` partitioner (`auto`, `simple`,
`static`), `i32` P, `vec<i64>` parameters (begin, end, grain; for `2d` the rows' and then the
columns'), `vec<i64>` the leaves (begin and end of each; for `2d` rows then columns).

### TBBR

A reduction's tree: `str` algorithm (`reduce`, `reduce_fn`, `det_reduce`, `det_reduce_fn`), `str`
partitioner (`default` for the overload without one), `i32` P, `i64` n, `u64` grain, `str` the tree
the bodies built: `[b,e)` for a leaf, `(l+r)` for a join, `I` for the functional forms' identity.

### TBBO

An order: `str` what (`for_index`, `for_each`, `invoke`, `task_group`, `task_group_nested`,
`task_group_for`), `str` variant (`for_index`: `default`, `static`, `simple`, `nostep`;
`for_each`: `iterators`, `range`), `i32` P, `vec<i64>` parameters (`for_index`: first, last,
step; the others: the count), `vec<i32>` the order the functions or indices ran in (`task_group`
pushes -1 when its `run` calls are done, before `wait`).

## Versions

A change to any record, its fields or their order is a new version. The reader refuses a version it
does not know.
