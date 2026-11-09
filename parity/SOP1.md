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
| `front-objects-<n>.sop1` | in the front end, once the objects are placed (after arranging and the build volume check) | HEAD, RUN, a CONF `run`, PCGC, then for each model object: MODL, a CONF `model-object`, a CONF `model-volume` for each of its volumes, HRNG, a CONF `model-height-range` for each of its height ranges, BRIM, and a PNTV for each of its volumes; END. The CONF `run`, PCGC, HRNG, the `model-height-range` CONFs, BRIM and PNTV are oracle patch 0006's (2026-11-07) |
| `front-config-<n>.sop1` | in the front end, once the config is resolved and applied to the print, before each slice | HEAD, RUN, CONF `job`, CONF `print-full`, CONF `print`, then for each print object a CONF `object` and a CONF `region` for each of its printing regions; END |
| `object<i>-<step>-<n>.sop1` | after the print object `i` (its index in the print) finishes the step `<step>` (`PrintObjectBaseWithState::set_done`) | HEAD, RUN, OBJ, a LAYR for each layer, a SUPL for each support layer; END |
| `print-<step>-<n>.sop1` | after the print finishes the step `<step>` (`PrintBaseWithState::set_done`) | HEAD, RUN, PRNT, TOOL, WIPE, STAT, CNFL; END |

The object steps are `posSlice`, `posPerimeters`, `posEstimateCurledExtrusions`,
`posPrepareInfill`, `posInfill`, `posIroning`, `posContouring`, `posSupportMaterial`,
`posSimplifyPath`, `posSimplifySupportPath`, `posDetectOverhangsForLift`, `posSimplifyWall` and
`posSimplifyInfill`. The print steps are `psWipeTower`, `psSkirtBrim`, `psGCodeExport` and
`psConflictCheck`. A step the engine skips writes nothing. Dumps are written one at a time, each
whole, under one lock.

`parity record` keeps the dumps of the boundaries in `KEPT_DUMPS` (`scripts/lib/parity/record.ts`;
`front-objects` in phase O2) of each oracle pass beside its runs, and `parity certify` compares the
Rust program's dumps of those boundaries with them byte for byte. Paths differ between the folders
a dump is written in, so before two dumps are compared each side's folders are written as
`<case>`, `<cases>`, `<resources>` and `<work>` inside the strings of HEAD, RUN, CONF, MODL and PCGC,
each string's length and its record's length written again (`neutral.ts` `portableDump`); the
kinds without a string field that can hold a path are compared as they are.

## The container

```
magic     4 bytes  "SOP1"
version   u32      1
records   ...      each: kind (4 bytes), length (u64), payload (length bytes)
END       record   kind "END ", payload one u64: the number of records before it
```

A kind is four ASCII letters, padded with spaces: `HEAD`, `RUN `, `CONF`, `MODL`, `PCGC`, `HRNG`,
`BRIM`, `PNTV`, `OBJ `, `LAYR`, `SUPL`, `PRNT`, `TOOL`, `WIPE`, `STAT`, `CNFL`, `END `. Nothing follows END. A reader refuses a file
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
| `run` | the front end's config once the objects are placed: `load_settings`' config with `place_prime_tower`'s `wipe_tower_x` and `wipe_tower_y` (patch 0006) | -1 | -1 |
| `model-object` | the model object's own settings | model object | -1 |
| `model-volume` | a volume's own settings | model object | volume |
| `model-height-range` | a height range's settings (`layer_config_ranges`, patch 0006) | model object | range, in HRNG's order |
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

### PCGC

The plates' custom G-code (oracle patch 0006): `i32` the current plate
(`Model::curr_plate_index`), then a `vec` of plates (`Model::plates_custom_gcodes`, by plate
index), each: `i32` plate index, `i32` mode (`CustomGCode::Mode`), `vec` of items, each: `f64`
print z, `i32` type (`CustomGCode::Type`), `i32` extruder, `str` colour, `str` extra (a pause's
message or a custom G-code).

### HRNG

A model object's height ranges (oracle patch 0006): `i64` object index, then a `vec` of ranges in
the order `layer_config_ranges` holds them (by from, then to), each: `f64` from, `f64` to. A CONF
`model-height-range` follows for each range, with its settings.

### BRIM

A model object's brim ears (`ModelObject::brim_points`, oracle patch 0006): `i64` object index,
then a `vec` of ears, each: `f32` x, `f32` y, `f32` z (its `pos`), `f32` head front radius.

### PNTV

A volume's painting (oracle patch 0006): `i64` object index, `i64` volume index, then four
paintings in this order: supports (`supported_facets`), seams (`seam_facets`), colours
(`mmu_segmentation_facets`) and fuzzy skin (`fuzzy_skin_facets`). Each is the
`TriangleSelector::TriangleSplittingData` the volume holds: a `vec` of split triangles, each `i32`
triangle index and `i32` index of its first bit; `u64` the bitstream's length in bits; a `vec<u8>`
of the bits, eight to a byte, the first bit in the lowest bit of the first byte (the last byte's
unused bits 0); and a `vec<bool>` of the states used (`used_states`, one per
`EnforcerBlockerType` up to `ExtruderMax`). An unpainted volume has empty vectors and its states
all 0.

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
| `crt.sop1` | O1, QSRT o2-geom | MATH, MATS, CAST, RAND, QSRT |
| `fmt.sop1` | O1 | PRTF, TOCH, OSTR, TOST, PARS |
| `stl.sop1` | O1 | SORT, VALS, HASH, UMAP, USET, RNGS, SSEQ, SHUF |
| `tbb.sop1` | O1 | TBBA, TBBF, TBBR, TBBO |
| `configdef.json` | O1 | not SOP1: `print_config_def` with every field of each option, the preset sections' keys, and `handle_legacy` on the legacy and defined keys (`configdef.cpp` says what it holds) |
| `geom.sop1` | o2-geom | EVEC, EMAT, ETRF, EROT, ESVD (`orca-eigen`); CCLP, COFF, CARE, CCLN, CSIM, CMNK, CINT, GPNT, GPLY, GCLU, GXFM (`orca-geom`) |
| `qhull.sop1` | o2-mesh | QHUL (`orca-qhull`; `orca-mesh` replays it too) |
| `mesh.sop1` | o2-mesh | MCAS, MSTD (`orca-mesh`) |
| `cstdio.sop1` | o2-mesh | CSCN, CFIL (`orca-fmt`'s `stdio` and `scanf`) |
| `zip.sop1` | o2-model | ZCRC, ZINF, ZARC, ZERR (`orca-zip`) |
| `xml.sop1` | o2-model | XPRS, XERR (`orca-xml`) |
| `model.sop1` | o2-model | MXFM, MFAC, MCFG, MCIR, MBVL, MLOD, MARP, MTMP (`orca-model`) |
| `arrange.sop1` | o2-arrange | XI28, XRAT (`orca-exact`); NLOP, NOPT, RTRE, NFPC, NGEO, ARNG (`orca-arrange`) |
| `placeholder.sop1` | o2-front | PHCF, PHEV (`orca-placeholder`) |

A stub is HEAD, PRUN and END: the task that fills it adds its records here, and the crate's probe
test fails as soon as the file holds a record it does not replay.

Inputs come from generators with fixed seeds, so two runs write the same bytes (`parity probe
--check`): the splitmix64 of `probe.hpp`, or a `std::mt19937` in the Clipper and Geometry records
of `geom.sop1` (seeds 0xC1199E2 and 0x6E0E7A1). Where a section has a value as a `u64`, a
`double` or `float` is its bit pattern, a `float`'s zero-extended, and an integer is sign-extended;
the Clipper and Geometry records write each value in its own width (`i64`, `i32`, `u8`, `f64`,
`f32`), as their sections say.

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

### QSRT

`qsort` of the static UCRT, as Qhull calls it (added 2026-11-07): `u32` comparator (0: `a - b`,
as `qh_intcompare` and `qh_comparemerge`; 1: `a > b ? 1 : -1`, never 0, as `qh_compareangle`; 2:
-1, 0 or 1, as `qh_compare_facetarea`), `u32` element size (4: a `uint32_t` with the key in its
high 16 bits and the id in its low 16; 8: an `int32_t` key, then a `uint32_t` id; 16: those, then
a `uint64_t`, the id times 0x9E3779B97F4A7C15), `vec<i32>` the keys (the element at index i has
id i), `u64` calls, then for each comparator call in order `u16` the first element's id, `u16` the
second's, `u16` the first's place in the array, `u16` the second's (65535 for a pointer that is
not at an element of the array), `u16` the result's low 16 bits (an `i16`); then `vec<u32>` the
ids in the order `qsort` left them. Each size has nine key arrays with ties (seeded keys below 1,
2, 3, 7 and 1000; pairs rising, pairs falling, rising then falling, alternating), each sorted with
the three comparators; elements of 8 bytes take sizes up to 1000 (the committed set up to 33), of
4 and 16 bytes up to 257 (the committed set up to 13).

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

### EVEC

A fixed-size vector's operations, as the engine's Eigen 5.0.1 computes them
(`parity/oracle/probe/geom/eigen.cpp`, added 2026-11-07): `u8` type (0 `double`, 1 `float`, 2
`int64_t`, 3 `int32_t`), `u8` size N (2, 3 or 4; the integer types 2 and 3), then each value a
`u64`: a's N coefficients and c's N; `a.dot(c)`, `squaredNorm()`, `sum()`, `prod()`,
`minCoeff()`, `maxCoeff()`, `(a - c).squaredNorm()`, the indices `minCoeff(&i)` and `maxCoeff(&i)`
give; N each of `cwiseMin(c)`, `cwiseMax(c)`, `cwiseAbs()`, `cwiseProduct(c)`, `a + c`, `a - c`;
for `double` and `float` then `norm()`, N of `normalized()`, `isApprox(c, 1e-3)` (0 or 1), N of
`a * 0.375` and N of `a / 3`; for N = 3 the 3 of `cross(c)`, for N = 2 the scalar `cross(c)`; last
N each of the casts to `double`, `float`, `int64_t` and `int32_t`. Coefficients are wide (`ldexp`
of a 53-bit mantissa, exponents -30 to 30, either sign, so that the order of additions shows),
small integers or signed zeros; the floating types also take infinities and the default quiet
NaN, never another NaN. 512 cases of each floating size and 256 of each integer size (the
committed set 8 and 4).

### EMAT

`u8` type (0 `double`, 1 `float`), `u8` N (`double` 2, 3, 4; `float` 3, 4), then A's N²
coefficients column by column, C's, and v's N; then `A * C`, `A * v`, `A * C.transpose()`,
`-A * v`, `A * v.asDiagonal()`, `v.asDiagonal() * A`, `sum()`, `squaredNorm()`, `trace()`,
`isDiagonal()` (0 or 1), and for N = 3 `determinant()` and the 9 of `inverse()`. Finite inputs, as
EVEC's. 256 or 512 cases a size (the committed set 4 or 8).

### ETRF

`Transform3d` (`Transform<double, 3, Affine, DontAlign>`): seven 3-vectors in, t1, r1, s1, t2, r2,
s2, pt; a and c are built as `Geometry::assemble_transform` builds them (from the identity:
`translate(t)`, `rotate(AngleAxis(r.z, Z) * AngleAxis(r.y, Y) * AngleAxis(r.x, X))`, `scale(s)`).
Then, matrices column by column: a's 16, c's 16, `a * c`'s 16, `a * pt`'s 3, `a.inverse()`'s 16,
`a.linear() * pt`'s 3, `computeRotationScaling`'s rotation (9) and scaling (9), the rotation's
`eulerAngles(2, 1, 0)` (3); a after `pretranslate(pt)`, after
`prerotate(AngleAxis(r2.x, X).toRotationMatrix())`, after `prescale(s2)` and after `scale(s2.x)`
(16 each); then as `Transform3f`, in `float`s: `a.cast<float>()` (16), that times
`pt.cast<float>()` (3), its `linear()` times the same (3), and it times `c.cast<float>()` (16).
Angles are 0, quarter turns or uniform in [-2π, 2π]; scales 1 or ±(1 to 2)·2^(-4 to 4). 512 cases
(the committed set 6).

### EROT

`f64` a1, `f64` a2, two unit axes (3 each), `f64` tiny (2^-60 to 2^-99), a 3x3 matrix rnd (9).
Then: r1 = `AngleAxisd(a1, ax1).toRotationMatrix()` (9); q1 = `Quaterniond(aa1)`'s coefficients
(x, y, z, w); `q1 * q2`; `aa1 * aa2`; `Quaternionf(af1) * Quaternionf(af2)` (4 `float`s, the
angle-axes cast); `q1.toRotationMatrix()` (9); `Quaterniond(r1)` and `Quaterniond(rnd)` (4 each);
the angle and the axis (1 and 3) of `AngleAxisd(q1)`, of `AngleAxisd(r1)` and of
`AngleAxisd(Quaterniond(1, tiny, -tiny * 0.5, tiny * 0.25))`; `Quaterniond(rnd(0,0), rnd(1,0),
rnd(2,0), rnd(0,1)).normalized()` (4); r1's `eulerAngles(2, 1, 0)`, `(0, 1, 2)` and `(2, 0, 2)`
and rnd's `eulerAngles(1, 0, 1)` (3 each). 512 cases (the committed set 8).

### ESVD

`JacobiSVD<Matrix3d>(m, ComputeFullU | ComputeFullV)`: m's 9 (a diagonal matrix, a rotation times
a scaling as `assemble_transform` makes them, or wide coefficients), then U's 9, V's 9, the 3
singular values and `u64` `nonzeroSingularValues()`. 1024 cases (the committed set 12).

### CCLP

A `Clipper` run, as libslic3r compiles Clipper 6.4.2 (`parity/oracle/probe/geom/clipper.cpp`,
added 2026-11-07). A path is a `u32` count and each point's `i64` x and y; a list is a `u32`
count and its paths; a tree is the root's contour, `i32` `Total()`, then the root's children: a
`u32` count, each child `u8` `IsOpen()`, `u8` `IsHole()`, its contour and its own children. The
case: `u8` initOptions (ioReverseSolution 1, ioStrictlySimple 2, ioPreserveCollinear 4), `u8`
ClipType (intersection, union, difference, xor), `u8` the subject's and `u8` the clip's
PolyFillType (even-odd, non-zero, positive, negative), `u8` tree (a PolyTree result), `u8` each
(`AddPath` per path, else `AddPaths` per group), `u8` twice (`Execute` twice on one Clipper), `u8`
convert (`PolyTreeToPaths`, `ClosedPathsFromPolyTree` and `OpenPathsFromPolyTree` of each tree);
`u32` groups, each `u8` PolyType (subject, clip), `u8` closed and its list. Then each add call's
result: `u8` 0 or 1, or 2 and `str` `what()` when it threw, which ends the record; `GetBounds()`
(`i64` left, top, right, bottom) when no group is open; each `Execute`: with a tree `u8` its result,
the tree and, with convert, the three lists; without, `u8` its result (or 2 and `what()`, which
ends the record) and the solution. Hand-made cases first (collinear, duplicate and spike points,
bow-ties, holes, touching squares, open and flat paths, coordinates at loRange and hiRange and past
them, `INT64_MIN`), then 3000 from a fixed-seed `std::mt19937` (the committed set the hand-made
cases and the first 30 others of at most 4 KB).

### COFF

A `ClipperOffset` run: `f64` MiterLimit, ArcTolerance and ShortestEdgeLength; `u32` paths, each
`u8` JoinType (square, round, miter), `u8` EndType (closed polygon, closed line, open butt, open
square, open round) and the path; `u32` runs, each `f64` delta and `u8` tree; then each run's `u8`
1 and its solution (a tree or a list), or 2 and `str` `what()`, which ends the record. Hand-made
cases (every join with every end type, one-point and two-point paths, spikes, holes, deltas of 0,
1e-21 and below 0.125 with ArcTolerance 0, where the step count is NaN, large ArcTolerances,
coordinates near hiRange), then 1500 random (the committed set those and the first 12 of at most
4 KB).

### CARE

The path, `f64` `Area()`, `u8` `Orientation()`, then a `u32` count of points, each point and `i32`
`PointInPolygon()` (1 inside, 0 outside, -1 on the boundary). 8 hand-made cases, then 1000 random
(the committed set those and 16).

### CCLN

`CleanPolygon`: the path, `f64` distance, the cleaned path. 7 hand-made cases, then 800 random
(the committed set those and 12).

### CSIM

`u8` 0 for `SimplifyPolygon` of the list's one path or 1 for `SimplifyPolygons`, the list, `u8`
PolyFillType, `u8` strictly simple, the result. 3 hand-made cases, then 600 random (the committed
set those and 8).

### CMNK

`u8` 0 for `MinkowskiSum` of a path, 1 for `MinkowskiSum` of a list or 2 for `MinkowskiDiff`, `u8`
closed, the pattern, the list (kinds 0 and 2 take its first path), the result. 4 hand-made cases,
then 400 random (the committed set those and 6).

### CINT

`Int128`'s filtered predicates, which Clipper's `SlopesEqual` and `Point.cpp` call (in the MSVC
build the exact ones they fall back on are private): `i64` a, b, c and d, then `i32`
`sign_determinant_2x2_filtered(a, b, c, d)` and `i32` `compare_rationals_filtered(a, b, c, d)`. 64
hand-made cases (values about 2^31 and 2^32, loRange, hiRange, `INT64_MIN`, `INT64_MAX`), then 4000
random (wide values and nearly collinear quadruples; the committed set those and 32).

### GPNT

libslic3r's `Point`, `Line` and `Geometry` predicates (`parity/oracle/probe/geom/geometry.cpp`,
added 2026-11-07). As in CCLP, a point is `i64` x and y and a path a `u32` count and its points;
here a `Vec2d` is two `f64`, a `Vec3d` three, and "a point if" is `u8` the call's result and the
point it gave, `(0, 0)` when it returned false. Inputs: points p, a, b, c, d and center, `f64`
angle and `f64` dmax; l is `Line(a, b)`, m `Line(c, d)`. Results: p after `rotate(angle)`, then p
after `rotate(angle, center)`; `f64` `p.ccw(a, b)` and `p.ccw_angle(a, b)`; `p.projection_onto(l)`; `f64`
`l.distance_to(p)`, `Line::distance_to_infinite(p, a, b)` and `l.perp_distance_to(p)`;
`l.intersection(m)` and `l.intersection_infinite(m)` as points if; `f64` `l.direction()`,
`orientation()` and `atan2_()`; `u8` `l.parallel_to(m)`, `perpendicular_to(m)`,
`parallel_to(angle)` and `perpendicular_to(angle)`; `l.midpoint()`, `l.normal()`; a and b of l
after `extend(dmax)`; `u8` `l.overlap(m, len)` and `f64` len (0 when false); `i32`
`Geometry::orient(a, b, p)`; `u8` `segments_intersect(a, b, c, d)`; `u8` `directions_parallel`
and `directions_perpendicular` of angle and `m.direction()` within `dmax * 1e-9`; `f64`
`to_range_pi_pi(angle * 7)` and `angle_to_0_2PI(angle)`; `f64` `l.distance_to_squared(p,
&closest)` and closest; `f64` `l.distance_to_infinite_squared(p, &closest)` and closest; `i32`
`p.nearest_point_index({a, b, c, d})`; `ray_ray_intersection` and `segment_segment_intersection`
of (a, b - a) and (c, d - c), each `u8` and the `Vec2d` (0, 0 when false); `foot_pt(l, p)` as a
`Vec2d`; `f64` `ray_point_distance(l, p)`. 5 hand-made cases (lines of one point, a quarter turn,
collinear segments, coordinates near 6e9, the angle 0.49999999999999994 with overlapping
segments), then 3000 random, every point within a range of 50, 1000, 30000, 1e6, 1e8 or 1e10
(past `orient`'s 32-bit branch), now and then with b = a, parallel lines or c on l (the committed
set those and the first 40 of at most 1 KB).

### GPLY

A point set as `Polygon`, `Polyline`, `MultiPoint` and `BoundingBox`. A list of polygons or
polylines is a `u32` count and each one's points; a box is min, max and `u8` defined. Inputs:
the points, points q, a and b, `f64` tol, frac, angle and factor; poly is `Polygon(points)`, pl
`Polyline(points)`, l `Line(a, b)`, dist `frac * max(1, poly.length()) * 0.25`. Results: `f64`
`area()` and `length()`; `centroid()`; `u8` `is_counter_clockwise()` and `Geometry::is_ccw`; `u8`
`contains(q)` and `Slic3r::contains(poly, q, false)`; `point_projection(q)`; the points after
`douglas_peucker(tol)`; the list `simplify(tol)`; the list `triangulate_convex`; `u8`
`polygon_is_convex`; the points after `remove_collinear`; `u8` and the points of `remove_sticks`
and of `remove_same_neighbor`; bb = `bounding_box()`, then bb after `offset(dist)`, after
`scale(factor)`; bb's `center()`, `size()`, `f64` `radius()`, `u8` `contains(q)`, box
`rotated(angle)` and `polygon()`'s points; `first_intersection(l)` as a point if; `u8`
`intersections(l, &pts)` and the points; `intersection(l)` as a point if; `u8` `on_boundary(q,
dist)`; `q.projection_onto(poly)`; `f64` `pl.length()`; pl's points after `clip_end(dist)`,
`clip_start(dist)` and `extend_end(dist)`; with spacing `max(pl.length() / (2 + 18 * frac), 1)`,
`equally_spaced_points(spacing)` and the list `equally_spaced_lines(spacing)`; the points after
`simplify(tol)`; `u8` `split_at_length(pl.length() * frac)` and the two parts' points (empty when
false); `leftmost_point()`; `u8` `is_straight()`; `i32` and point of `foot_pt(points, q)`; `u8`
`has_duplicate_points(points)`; `Geometry::convex_hull(points)`'s points; the list
`polygons_simplify({poly}, tol, true)`; box `get_extents_rotated(points, angle)`; poly's points
after `rotate(angle, q)`, after `rotate(angle)`, after `scale(factor)`;
`MultiPoint::_douglas_peucker(points, tol)`; `poly.equally_spaced_points(spacing)`. 6 hand-made
cases (a square, a rectangle, three collinear points, duplicate points, a bow-tie, a stick), then
1500 random: star polygons, clockwise stars and point soups of 3 to 24 points, now and then with a
repeated or a collinear point, in GPNT's ranges (the committed set those and the first 12 of at
most 4 KB).

### GCLU

ClipperUtils and `ExPolygon`. A list of ExPolygons is a `u32` count, each the contour's points
and the holes as a list of polygons. Inputs: polygon lists A and B, `f32` delta, `u8` join
(square, round, miter), `f64` the miter limit (the arc tolerance
for round), `u8` safety (`ApplySafetyOffset::Yes`), a polyline's points, point q. With pos
`fabs(delta) + 1.f`: the lists `offset(A, delta)` and `offset_ex(A, delta)`, `union_(A)`, EA =
`union_ex(A)` (EB = `union_ex(B)` is not written); `diff(A, B)`, `diff_ex`, `intersection`,
`intersection_ex`; `xor_ex(EA, EB)`, `diff_ex(EA, EB)`; `offset_ex(EA, delta)`, `offset(EA,
delta)`, `offset2_ex(EA, delta, -0.5f * delta)`; `closing(A, pos)`, `opening(A, pos)`;
`diff_pl({pl}, A)`, `intersection_pl({pl}, A)`, `diff_pl(A, B)`; `offset(pl, pos)` with the line
defaults (square, 0, open butt); `simplify_polygons(A)`, `union_safety_offset(A)`,
`contour_to_polygons(A, 2.f * pos)`; `diff_clipped(A, B)`, `intersection_clipped(A, B)`;
`top_level_islands(A then B)`; `ClipperUtils::clip_clipper_polygons_with_subject_bbox(B,
get_extents(A))`. When EA is not empty, of its first ExPolygon e: `f64` `area()`, `u8`
`contains(q)` and `is_valid()`, `point_projection(q)`, `u8` `overlaps(EB[0])` (0 when EB is
empty); then the variable offset deltas, drawn once e is known: for each contour and hole, a
`u32` count and that many `f32` in [0, pos), then the same for the inner deltas in
(-pos, 0]; and the lists `variable_offset_outer(e, outer)`, `variable_offset_inner(e, inner)`,
`variable_offset_outer_ex`, `variable_offset_inner_ex`. The safety, join type and miter limit
apply to every call that takes them. 2 hand-made cases (overlapping squares, a square with a
hole), then 600 random (1 to 3 polygons each, as GPLY's, in ranges 1e5 and 1e6; the committed set
those and the first 4 of at most 12 KB).

### GXFM

The transform builders and the hulls. Inputs: `Vec3d` t, r, s and m (each of m ±1), a list of
`Vec3d` (`u32` count), the point soups sa and sb, `f64` radius, error and angle. Results: tr =
`Geometry::assemble_transform(t, r, s, m)` and `rotation_transform(r)`, each as the 16 `f64` of
its matrix row by row; `extract_euler_angles(tr)` (the `Transform3d` overload) and
`extract_euler_angles` of `rotation_transform(r)`'s 3x3 block; the min and max of
`BoundingBoxf3(list).transformed(tr)`; `Geometry::convex_hull(list)`, the hull in x and y (a
list of `Vec3d`); `make_circle(radius, error)`'s points; `f64` `rad2deg_dir(angle)`; the points of ha =
`convex_hull(sa)` and hb = `convex_hull(sb)`; `u8` `convex_polygons_intersect(ha, hb)` when both
have 3 points or more (0 otherwise); the bottom and the top chain of
`decompose_convex_polygon_top_bottom` of ha's points unscaled (`double(x) * SCALING_FACTOR`), each
a `u32` count and its `Vec2d`s; then for each point of sb, no count, `u8` `inside_convex_polygon`
of it unscaled (0 when the bottom chain is empty). 2 hand-made cases (the identity with
overlapping squares; a mirrored, scaled quarter-turn transform with squares apart), then 1500
random: 3 to 20 points in [-100, 100] cubed, now and then one more 1 above the first; soups of 3
to 16 points in GPNT's ranges capped at 1e8, as `convex_polygons_intersect` loops forever on some
larger ones (`parity/HAZARDS.md`); the committed set those and the first 16 of at most 3 KB.

### CSCN

`sscanf` of the static UCRT with one of the formats admesh's STL reader passes (`cstdio.sop1`,
`parity/oracle/probe/cstdio.cpp`, added 2026-11-07): `u8` the format's index, `str` the input,
`i32` what `sscanf` returned, then each argument after the call, in the format's order: a `float`
as `u32` bits, a `double` as `u64` bits, a `char[32]` (`%31s`) or a `char[256]` (`%[^\n]`) as a
`str` of all its bytes. Each argument holds a sentinel before the call (the `float` bits
0x12345678, the `double` bits 0x123456789ABCDEF0, the byte 0x55 throughout), so that a conversion
that stores nothing shows. The formats, by index: 0 `"%f"`, 1 `"%lf"`, 2 `" vertex %f %f %f"`, 3
`" vertex %f %f %f "`, 4 `" facet normal %31s %31s %31s"`, 5 `" outer loop"`, 6
`" endsolid%*[^\n]\n"`, 7 `" solid%*[^\n]\n"`, 8 `" solid %[^\n]"`, 9 `" "`, 10 `"%f %f %f"`.
The inputs: 108 spellings of a number with formats 0 and 1 (whole ones, ones `strtod` stops inside,
hexadecimal forms, infinities and NaNs with payloads, the limits of `float` and `double`, long
digit strings, white space before), then 600 seeded inputs for each format, text that matches it
or nearly does (the committed set the 108 twice and the first 40 of each format).
`orca-fmt`'s `tests/probes.rs` replays every record: the count and every byte of every argument.

### CFIL

A file and a script of stdio calls on it (`cstdio.sop1`, added 2026-11-07): `str` the file's
bytes, `u8` the mode it was opened in (0 `"rb"`, 1 `"r"`, 2 `"rt"`), `u32` the number of calls,
then for each call `u8` what it is and what it gave: 0 `getc`, `i32` the result; 1
`fgets(buf, n)` with `buf` filled with 0x55 first, `i32` n, `bool` the result was not null, `str`
the buffer up to and with its first NUL (all n bytes when it holds none); 2 `fscanf` with a CSCN
format, `u8` its index, `i32` the result and the arguments as CSCN writes them; 3
`fread(buf, size, count)` with `buf` filled with 0x55 first, `u32` size, `u32` count, `u64` the
result, `str` the buffer: `size * count` bytes for a binary stream, the result times `size` for a
text stream (asked for 512 bytes or more at a text stream's start, the UCRT leaves raw file bytes
after what it returns, which C leaves unspecified: `parity/HAZARDS.md`, orca-mesh); 4 `ftell`,
`i32`; 5 `fseek(offset, whence)`, `i32` offset, `i32` whence, `i32` the result; 6 `rewind`,
nothing; 7 `feof`, `bool`; 8 nothing: the reader's `fscanf(" solid %[^\n]")` at a `solid` line of
256 bytes or more, which would overrun the C buffer, so the probe leaves the call out and the
replay's call must stop with `Overflow`. Text streams take no `ftell` and no `fseek` but to the
start (no reader calls them, and `orca-fmt` leaves them out); a binary stream's `fseek` never goes
before the start. The scripts: the calls admesh's reader makes of an ASCII STL (the size by
`fseek` and `ftell` in binary mode, the lines counted by `fgets(100)`, the header by `getc`, the
`solid` line, then three facets' `fscanf` and `fgets(2047)` calls) on STL text with each line end
(LF, CR LF, CR, CR CR LF, LF CR) in each mode; the same with mixed line ends and a Ctrl-Z inside,
and on the files "", "solid", Ctrl-Z, CR and "a CR Ctrl-Z LF b", each with seeded calls after;
then 1500 seeded scripts, half STL text (now and then with a Ctrl-Z or cut short) read as admesh
reads it with seeded calls after, half random bytes (CR, LF and Ctrl-Z often, up to 9000 bytes)
with seeded calls. The committed set holds the fixed scripts and the first 30 seeded ones of at
most 8 KB. `orca-fmt`'s `tests/probes.rs` replays every call of every record.

### MCAS

One STL file through admesh and `TriangleMesh` (`mesh.sop1`, `parity/oracle/probe/mesh.cpp`,
added 2026-11-07), under the oracle's threading (one thread in an arena of 8): `str` label, `str`
the file's bytes, `i32` the header length passed to `stl_open` (`custom_header_length`), `bool`
`stl_open` succeeded. When it did: `u32` the number of steps, then for each `u8` the step and
`u64` the hash of admesh's state after it (the bytes MSTD writes); the steps are 0 `stl_open`, then
those of `trianglemesh_repair_on_import` that run: 1 `stl_check_facets_exact` with the bad-edge
counts set after it, 2 and 3 the first and second `stl_check_facets_nearby`, 4
`stl_remove_unconnected_facets`, 5 `stl_fix_normal_directions`, 6 `stl_fix_normal_values`, 7
`stl_calculate_volume`, 8 `stl_verify_neighbors`, 9 `stl_check_facets_exact` again (after
degenerate facets); then `u64` the hash of `stl_generate_shared_vertices`'s indexed set. Then
`bool` `TriangleMesh::ReadSTLFile(path, true)` succeeded, and when it did: the mesh, `u64`
`facets_count()`, the box `bounding_box()`, three `f64` `size()`, three `f64` `center()`, `bool`
`empty()` and `is_splittable()`, `u64` `its_num_open_edges` and `its_number_of_patches`, the
polygon `convex_hull()`, `u32` the number of `split()`'s parts and each part as a mesh; then `u32`
the length of a seeded chain of calls on a copy of the mesh (0 for an empty mesh, else 6 to 19),
each `u8` the call, its inputs and its result: 0 `scale(float)`, `f32`, the mesh; 1
`scale(Vec3f)`, three `f32`, the mesh; 2 `translate(Vec3f)`, three `f32`, the mesh; 3
`rotate(float, Axis)`, `f32`, `u8` axis, the mesh; 4 `rotate(float, Vec3d)`, `f32`, three `f64`,
the mesh; 5 `mirror(Axis)`, `u8`, the mesh; 6 `transform(Transform3d, fix_left_handed)`, the
matrix's 16 `f64` row by row, `bool`, the mesh; 7 `transform(Matrix3d, fix_left_handed)`, nine
`f64`, `bool`, the mesh; 8 `flip_triangles()`, the mesh; 9 `align_to_origin()`, the mesh; 10
`merge` of the mesh as read, translated by a `Vec3f`: three `f32`, the mesh; 11
`transformed_bounding_box(t)`, 16 `f64`, the box; 12 `transformed_bounding_box(t, z)`, 16 `f64`,
`f64` z, the box; 13 `rotate(double, const Point*)`, `f64` angle, `i64` the center's x and y, the
mesh; 14 `convex_hull()`, the polygon. A mesh is `u32` `number_of_facets`, three `f32` each of
max, min and size, `f32` volume, `i32` `number_of_parts` and `open_edges`, the repaired errors
`i32` `edges_fixed`, `degenerate_facets`, `facets_removed`, `facets_reversed` and
`backwards_edges`, and `u64` the hash of its indexed set; a box is `bool` defined, three `f64` min
and three max; a polygon is a `u64` count and each point's `i64` x and y. A hash is FNV-1a (64
bits) of the bytes MSTD gives. The files: 28 made by hand (binary and ASCII cubes with a facet
flipped, removed, doubled or degenerate, a vertex nudged or NaN, inside out, two boxes, no facets,
four, cut short, a wrong count, a 100-byte header and header length 0, each line end but a lone
CR, no final newline, `%f` numbers, a Ctrl-Z at the end and inside, an `endloop` missing, NaN
normals, a second empty solid, too short), OrcaSlicer's five test STL files but `20mmbox-CR.stl`
(`parity/CORPUS.md`), then 700 seeded: boxes of 1 to 8 quads a side in one to three parts, rotated
or not, with facets flipped, removed, doubled or degenerate, vertices nudged, all inside out, a
NaN, shuffled; normals right, zero, reversed, random or NaN; attribute bytes; a third ASCII with
numbers as `%.9g`, `%e`, `%f` or `%.9E` and any line end but a lone CR, the binary ones now and
then with a wrong count or cut short (the committed set the hand-made and OrcaSlicer files and the
first 30 seeded ones of at most 4 KB). `orca-mesh`'s `tests/probes.rs` rebuilds every record from
the Rust and compares it whole.

### MSTD

The full set only, after each MCAS whose file `stl_open` read into 24 facets or fewer: `str` the
label, `u32` the number of steps, then for each `u8` the step and `str` admesh's state after it,
then `str` the indexed set of `stl_generate_shared_vertices`. The state: `str` the 80-byte
header, `u8` the type (0 binary, 1 ASCII), `u32` `number_of_facets`, three `f32` each of max, min
and size, `f32` `bounding_diameter`, `shortest_edge` and `volume`, `i32` `connected_edges`,
`connected_facets_1_edge`, `connected_facets_2_edge`, `connected_facets_3_edge`,
`facets_w_1_bad_edge`, `facets_w_2_bad_edge`, `facets_w_3_bad_edge`, `original_num_facets`,
`edges_fixed`, `degenerate_facets`, `facets_removed`, `facets_added`, `facets_reversed`,
`backwards_edges`, `normals_fixed` and `number_of_parts`; `u64` the length of `facet_start` and
for each facet twelve `f32` (the normal, then the vertices), then for a binary file its two extra
bytes as `u8` (an ASCII facet leaves them unset); `u64` the length of `neighbors_start` and for
each entry three `i32` neighbours and three `i32` `which_vertex_not`. Both arrays are written
whole, which after `stl_remove_unconnected_facets` is longer than `number_of_facets`. An indexed
set is `u64` the vertex count and each vertex's three `f32`, `u64` the face count and each face's
three `i32`, `u64` the property count and each property's `u8` type and `f64` area.
`orca-mesh`'s `tests/probes.rs` compares every MSTD whole, so that a difference an MCAS hash only
shows is named there down to the field, the facet or the neighbour entry.

### QHUL

One point set through libslic3r's `its_convex_hull`, which runs Qhull (`qhull.sop1`,
`parity/oracle/probe/qhull.cpp`, added 2026-11-07), under the oracle's threading (one thread in an
arena of 8): `str` label, `u64` the number of points and each point's three `f32`; then what
`its_convex_hull` returned: `u64` the number of vertices and each vertex's three `f32`, `u64` the
number of triangles and each triangle's three `i32` vertex indices, in the order its walk of
Qhull's facets gives them (both empty when Qhull throws, as for fewer than four points or a flat
set), `u64` the number of face properties (the two-argument `indexed_triangle_set` constructor
sizes them as the faces); and `str` the text `~Qhull` wrote to `std::cerr`, caught from the
stream, so with `\n` line ends (Qhull's narrow-hull warning, or nothing). The sets: 14 made by
hand (no points, one, three, four flat, five collinear, one point five times, a tetrahedron, a
flat one that warns of a narrow hull, a cube's corners, with a centre and face centres added, a
5x5x5 grid, a cube's corners with an infinite point, with a NaN point last, a NaN point first);
1200 seeded sets of 4 to 3003 points (one in 16 followed by a repeat of an earlier one) in a box,
a ball, a thin slab, on a sphere, a small grid or a box's faces, scaled and now and then moved far
off (seed 0x9A11F000); 400 of
tiny (10^-30 to 10^-8) and huge (10^8 to 10^30) coordinates, needles and box faces moved off by up
to 1e-6 (0x9A11F001); 800 lattice sets whose slanted faces hold exactly coplanar points
(0x9A11F002); 600 clusters with 3 to 8 points 10^6 to 10^11 away, whose facets are flat to
Qhull's precision (0x9A11F003); 600 prisms, cylinders, frustums and cones (0x9A11F004); then the
shared vertices of OrcaSlicer's five test STL files but `20mmbox-CR.stl` (`parity/CORPUS.md`):
3619 records. The committed set holds the hand-made sets, the STL files' and the first small
seeded sets of each generator (60 of at most 160 points, then 20, 40, 30 and 30 of at most 120):
199 records. `orca-qhull`'s `tests/probes.rs` replays every record through `convex_hull` and the
walk, and `orca-mesh`'s through `its_convex_hull` and `TriangleMesh::convex_hull_3d`, field for
field and bit for bit.

### PHCF

A configuration the PHEV records evaluate against (`placeholder.sop1`): `u32` id, `str` name, then
a `vec` of options, each `str` key and `str` value as `ConfigOption::serialize` writes it. Only the
keys the expressions name are listed (every identifier of the configuration's expressions, with
each percentage's `ratio_over` chain), in key order. Ids 0 and 1 are the hand-written cases'
configuration (a printer's and a process's defaults with a few values set) and its printer
override (`printer_preset` "Test Printer", `num_extruders` its nozzle count). In the full set,
each instantiated printer of a vendor profile (`<vendor>/machine/<name>`, the type's defaults then
its `inherits` chain read with `load_from_json`, the root first) is followed by its override
(`<vendor>/override/<name>`, as `is_compatible_with_printer` builds it), and each instantiated
process (`<vendor>/process/<name>`) has no override. The vendor profiles are those of the oracle's
source tree, found from the probe's own path (`<exe>/../src/resources/profiles`).

### PHEV

One `PlaceholderParser::evaluate_boolean_expression(template, config, override)`: `u32` the
config's PHCF id, `i32` the override's PHCF id (or -1 for none), `str` the template, `u8` outcome
(0 a result, 1 `PlaceholderParserError`, 2 another `std::exception`, 3 anything else), `bool` the
result (false unless the outcome is 0), `str` the exception's `what()` (empty for 0 and 3). Both
sets have the hand-written cases twice, without and with the override (about 370 expressions:
every operator, function, type error, regular expression form and regex error, and syntax errors
at many places, invalid UTF-8 among them). The full set adds every distinct
`compatible_printers_condition` of a vendor's processes and filaments against each of its
printers with the override, and every `compatible_prints_condition` of its filaments against each
of its processes. `orca-placeholder`'s `tests/probes.rs` replays every PHEV: a result or a
`PlaceholderParserError`'s message must match exactly, and outcomes 2 and 3 must be declined
(`Unsupported`).

### arrange.sop1's values

The records of `arrange.sop1` (`parity/oracle/probe/arrange.cpp` and `arrange/`, added
2026-11-07) write a point list as a `u32` count and each point's `i64` x and y; a shape (an
`ExPolygon`) as its contour's point list, a `u32` hole count and each hole's point list; a box as
its minimum and its maximum `point`; and an `int128_t` (`i128` below) as `bool` negative, `u64`
the high and `u64` the low half of its magnitude. A call that may throw is `bool` threw, then the
exception's `str` `what()` or the value. Each record holds its inputs, then the engine's results,
so the replays need no generator. `orca-exact`'s `tests/probes.rs` replays XI28 and XRAT,
`orca-arrange`'s the rest; each compares every result bit for bit.

### XI28

Boost.Multiprecision's `int128_t`, the `LargeInt` of `Arrange.cpp` under MSVC
(`arrange/exact.cpp`): `i128` a and b, then `i128` a + b, a - b and a * b; a / b, a call that may
throw; a % b, a call that may throw, only when b is not zero (a remainder by zero ends the C++
process: `parity/HAZARDS.md`, orca-exact); `i128` -a and abs(a); gcd(a, b), a call that may throw
(`boost::integer::gcd`, which is Boost.Multiprecision's `eval_gcd` here); `f64` double(a); `bool`
a < b, a == b and a > b. The cases: 54 pairs of 18 chosen values (0, ±1, 2, -3, 7, ±2^63,
±(2^64 - 1), ±2^64, ±2^127, ±(2^128 - 1), 2^127 - 1 and 1000000007), then 4000 seeded pairs of
magnitudes of chosen sizes (zero, one, up to 1000, up to 64, 32, 62, 64, 96 or 128 bits, all
ones, a single bit), each signed at random. The committed set: the 54 and the first 120 seeded.

### XRAT

`boost::rational<int128_t>`, the ratio `nfpConvexOnly` sorts edges with (`arrange/exact.cpp`):
`i128` n1, d1, n2, d2 and i; then rational(n1, d1) and rational(n2, d2), calls that may throw (a
rational is `i128` numerator and denominator). When both returned: a + b, a - b, a * b, a / b, a
< b, a > b, a == b, a < i, a > i, a == i, a + i, a - i, a * i, a / i, -a and abs(a), each a call
that may throw (the comparisons' values `bool`), then `f64` `rational_cast<double>(a)`. The cases:
7 chosen (the edge sort's own pattern, zero denominators, the sign moves of `normalize()`, values
near 2^31 and 2^62, the largest), then 2500 seeded, mostly of the sizes the edge sort makes (a
coordinate difference over a squared length), now and then any 128-bit value. The committed set:
the 7 and the first 60 seeded.

### NLOP

NLopt 2.5.0 through `nlopt::opt`, as the engine links it (`arrange/nlopt.cpp`): `u8` the
algorithm (0 `LN_NELDERMEAD`, 1 `LN_SBPLX`), `u32` n, n `f64` x0, n `f64` lower and n `f64` upper
bounds, `bool` maximise, `u8` the setters that run (1 `stopval`, 2 `ftol_rel`, 4 `ftol_abs`, 8
`xtol_rel`, 16 `xtol_abs` with one value for all, 32 `maxeval`, 64 the initial step), `f64`
stopval, ftol_rel, ftol_abs, xtol_rel and xtol_abs, `i32` maxeval, n `f64` initial steps when the
setters have 64, `u8` the objective, four `f64` its parameters c, `i32` the evaluation (from 0)
whose objective calls `force_stop()` (-1 none). Then `bool` `set_initial_step` threw, and when it
did `u8` the kind and `str` `what()`, which end the record. Then `u32` the number of evaluations,
each n `f64` x and `f64` the value; `bool` `optimize` threw, then `u8` the kind and `str`
`what()`, or `i32` the result code; n `f64` the final x and `f64` the final value. The kinds: 1
`std::runtime_error`, 2 `std::invalid_argument`, 3 `std::bad_alloc`, 4 `nlopt::roundoff_limited`,
5 `nlopt::forced_stop`, 9 another `std::exception`. The objectives, by id, which the replay
computes in the same order: 0 the sum of (x[i] - c[i])^2; 1 Rosenbrock's sum of 100 (x[i+1] -
x[i]^2)^2 + (1 - x[i])^2, plus c[0]; 2 the sum of |x[i] - c[i]| (i + 1); 3 NaN where x[0] > c[1],
else 0's; 4 `HUGE_VAL` where x[0] < c[1], else 0's; 5 1 where x[0] > c[0], else 0, plus 0's times
1e-3. No time limit is ever set. The cases: 10 chosen (the placer's subplex in [0, 1] from a
corner and its Nelder-Mead twin, `minimizeCircle`'s two variables, a start outside the bounds, a
dimension with equal bounds eliminated, with and without a forced stop, a forced stop, a step too
small for the simplex, a zero step, a maximised NaN objective with a stop value), then 1500
seeded, each with an evaluation limit: 1 to 4 variables, bounds of each kind (none, one side,
equal, both) at three scales. The committed set: the 10 and the first 60 seeded of at most 2048
bytes.

### NOPT

libnest2d's `TOptimizer<L_SUBPLEX>::optimize_min` over 1, 2 or 3 variables (`arrange/nlopt.cpp`):
`u32` n, n `f64` x0, n `f64` lower and n `f64` upper bounds, `f64` the stop criteria's
`absolute_score_difference`, `relative_score_difference` and `stop_score` (NaN: not set), `u32`
`max_iterations`, `u8` the objective and four `f64` c (NLOP's), `i32` the evaluation (from 0)
that throws (-1 none) and `u8` its kind (NLOP's kinds 1 to 5: `std::runtime_error("probe
failure")`, `std::invalid_argument("probe invalid argument")`, `std::bad_alloc`,
`nlopt::roundoff_limited`, `nlopt::forced_stop`). Then `u32` the number of evaluations, each n
`f64` x, `u8` 1 when it threw and `f64` the value (0 when it threw); `bool` `optimize_min` threw,
then `u8` the kind and `str` `what()`, or `i32` the result code, n `f64` the optimum and `f64` the
score. The cases: 8 chosen (the placer's one variable in [0, 1], the same throwing each kind at
the fifth evaluation and a forced stop at the first, a start outside the bounds), then 1200
seeded. The committed set: the 8 and the first 50 seeded of at most 2048 bytes.

### RTRE

`Arrange.cpp`'s spatial index, `bgi::rtree<std::pair<Box, unsigned>, bgi::rstar<16, 4>>`
(`arrange/nest.cpp`): `u32` the number of operations, then each `u8` 0 insert, the box and `u32`
its index; 1 an `intersects` query, the box, `u32` the number of elements whose box meets it and
each one's box and index, sorted by index (`Arrange.cpp` never reads their order); 2 the box
`bounds()`, `u64` `size()` and `bool` `empty()`; 3 `clear()`. The cases: 400 sequences, the first
four of 8, 48, 88 and 128 operations, the rest of 1 to 60 or (every fifth) 1 to 400, inserting
boxes at one of five scales (some flat, some inserted twice) and querying boxes, some of them
touching an inserted box at a corner. The committed set: the first 12 of at most 4096 bytes.

### NFPC

`nfpConvexOnly<ExPolygon, boost::rational<LargeInt>>` and `nfpInnerRectBed` (`arrange/nest.cpp`):
the point lists sh and other, then `nfpConvexOnly(sh, other)`, a call that may throw whose value
is the shape and its reference `point`; then the bed box and `nfpInnerRectBed(bed, other)`: the
shape and its reference `point`. The cases: 3000, three chosen (two clockwise squares, as
`Arrange.cpp` makes its contours; a counter-clockwise triangle and a square; edges of one
direction in both), the rest seeded: convex shapes, stars, rectangles, Ls, slivers and point soups
at five scales, half of them replaced by their hulls, on beds about the origin up to 400 mm, now
and then smaller than the shape. The committed set: the first 80 of at most 1536 bytes.

### NGEO

libnest2d's item, hulls, predicates and edge cache (`arrange/nest.cpp`): the point lists pts and
other (other is simple), `bool` pts is simple, `bool` the rotation is set and `f64` the rotation,
`bool` the translation is set and the `point`, `bool` the inflation is set and `i64` the
inflation, a box, a circle's `point` centre and `f64` radius, a `point` pt, `f64` the edge cache's
accuracy, `u32` the number of distances and each `f64`. Then the item's transformed shape, `f64`
`area()`, the box `boundingBox()`, the `point`s `referenceVertex()` and `leftmostBottomVertex()`;
the shapes `convexHull` of the transformed shape and of it with other; `minimizeCircle` of the
transformed shape, `point` centre and `f64` radius; `bool` `isInside` of (shape, box), (shape,
circle), (bounding box, circle), (bounding box, box), (pt, circle), (pt, box) and (pt, shape), and
`touches(pt, shape)`; `within(hull, other)`, a call that may throw with a `bool` value, and, for a
simple pts, `within(shape, other)` the same; `f64` the placer's `overfit` of (bounding box, box)
and (shape, circle), and its `overfit(hull, other)`, a call that may throw with an `f64` value;
then `EdgeCache` of the transformed shape at the accuracy: `u32` the number of corners and each
`f64` corner, `f64` `circumference()`, each corner's `point` `coords` and each distance's. The
cases: 1500, two chosen (a rectangle in a larger square; a square inflated and moved, as
`Arrange.cpp`'s items are), the rest seeded: NFPC's shapes at five scales, each of rotation,
translation and inflation set or not, accuracies 1, 0.65, 0.5, 0.3, 0.1 and 0, up to five
distances in [0, 2.5]. The committed set: the first 30 of at most 3072 bytes.

### ARNG

One whole `Slic3r::arrangement::arrange` (`arrange/runs.cpp`). The bed: `u8` 0 bed points (the
`Points` overload, which makes a box, a circle, a polygon or an infinite bed of them), a point
list; 1 a `BoundingBox`, two `point`s; 2 a `CircleBed`, `point` centre and `f64` radius; 3 a
`Polygon`, a point list; 4 an `InfiniteBed`, `point` centre. The parameters: `i64`
`min_obj_distance`, `f32` `accuracy`, `bool` `parallel`, `allow_rotations`, `do_final_align`,
`allow_multi_materials_on_same_plate`, `avoid_extrusion_cali_region`, `is_seq_print` and
`align_to_y_axis`, `f32` `bed_shrink_x`, `bed_shrink_y`, `brim_skirt_distance`,
`clearance_height_to_rod`, `clearance_height_to_lid`, `clearance_radius`, `object_skirt_offset`,
`nozzle_height` and `printable_height`, `f64` `align_center`'s x and y. Then four lists of
`ArrangePolygon`s, `excluded_regions`, `nonprefered_regions`, the items and the excludes: each a
`u32` count and each polygon's contour point list, `i64` translation x and y, `f64` rotation,
`i64` inflation, `i32` `bed_idx` and `priority`, `bool` `is_virt_object` and `is_wipe_tower`,
`u32` the number of `extrude_ids` and each `i32`, `i32` `filament_temp_type`, `first_bed_temp`,
`print_temp`, `vitrify_temp` and `itemid`, `f64` `height`, `str` `name`. Then `bool` the progress
callbacks are set (`progressind` and `on_packed`; otherwise both are null) and `i32` the stop
condition's limit (-1: none; otherwise `stopcondition` says stop from its call limit + 1 on). The
results: `bool` `arrange` threw, and `str` `what()` when it did; for each item `i64` translation x
and y, `f64` rotation, `i32` `bed_idx` and `itemid`; `u32` the number of `progressind` calls and
each `u32` st and `str` name; `u32` the number of `on_packed` calls and each `i32` `bed_idx` and
`priority`; `u32` the stop condition's calls. The cases: 13 chosen (`cli/run.cpp`'s five parts on
a 256 mm bed with its excluded area and prime tower, and the same printed by object; a delta's
round bed from points; an irregular convex bed printed by object; an L-shaped bed through the
typed overload; infinite beds from no point, one point and the typed overload; the typed box and
circle overloads with an item too big, one not to arrange, one of two points and one on plate 3;
more parts than a plate holds, fixed parts on plates 0 and 1, priorities and filaments, stopped
part way and not; a user alignment point with a lower accuracy and serial placement), then 300
seeded: boxes, circles, convex or L-shaped polygons and infinite beds, each as points or through
its typed overload; 1 to 16 parts of every shape, some too big, moved, rotated or on other plates;
on boxes the CLI's excluded area and prime tower, fixed parts, and excluded and non-preferred
regions. The seeded runs keep to inputs where the C++ is defined and ends (`parity/HAZARDS.md`,
orca-arrange). The committed set: the 13 and the first 16 seeded of at most five items, an
accuracy of 0.65 or less and at most 6144 bytes.

### zip.sop1's values

The records of `zip.sop1` (`parity/oracle/probe/zip.cpp`, added 2026-11-07) write each value in
its own width. A hash is FNV-1a (64 bits) of the bytes. Streams come from miniz's own compressor
(levels 0 to 10, its strategies, raw and zlib, raw-only and static-only blocks), by hand, cut
short, with junk after them or with bits flipped; archives from miniz's writer at a fixed time and
from a builder in the probe that writes each field as told (ZIP64 records and extra fields, disk
numbers, flags, methods, sizes and CRCs that lie, comments, junk before), cut short or with bytes
changed. `orca-zip`'s `tests/probes.rs` rebuilds every record and compares it whole.

### ZCRC

`u32` the running value, `str` the bytes, `u32` `mz_crc32` of them. 48 cases; the committed set
those of 300 bytes or fewer.

### ZINF

`tinfl_decompress` called until it ends: the schedule, `u8` a ring (1) or a buffer that does not
wrap, `u64` its size, `u32` the flags (`TINFL_FLAG_PARSE_ZLIB_HEADER`,
`TINFL_FLAG_COMPUTE_ADLER32`), `u8` more input promised on every call, `u64` the seed of the
chunks, `u32` the largest input chunk (0: all that is left) and `u32` the largest output chunk (0:
all the room), `str` the stream; then `u64` the number of calls, the first 16 calls' `i32` status,
`u64` input taken and `u64` output given, `u64` the hash of every call's three, `i32` the last
status, `u64` all the input taken, `u64` all the output given, `u32` `tinfl_get_adler32`, `u64` the
hash of the output, `bool` it is 256 bytes or fewer and then `str` it. Each stream runs under eight
schedules (all at once, seeded chunks, a 32 KiB ring with all or seeded input, a byte at a time,
more input promised to the end, a ring of 1024 bytes and one of 1000, which is refused); the
committed set takes the small sources' first schedules.

### ZARC

One archive: `str` label, `str` its bytes, `u8` how it is opened (0 `open_zip_reader` of a file, 1
`mz_zip_reader_init_mem`, 2 `mz_zip_reader_init` with a read function, 3 a path that does not
exist, 4 a folder), `u32` the init flags, `u32` the extra field buffer's size, `u32` the callback
call that refuses one byte, `u64` the number of name queries and each `str` name, `u32` flags,
`bool` and `str` a comment. Then `bool` it opened, `i32` `m_last_error`, `str`
`MZ_Archive::get_errorstr()`, `str` `mz_zip_get_error_string`, `u32` the number of files, `u64`
`m_archive_size` and `m_central_directory_file_ofs`, `bool` `mz_zip_is_zip64`. For each entry up
to 40 and the one past the last: `bool` `mz_zip_reader_file_stat` and `i32` the error, the
statistics when it worked (`u32` index, `u64` directory offset, `u16` versions made by and
needed, bit flag and method, `u32` CRC, `u64` compressed and uncompressed sizes, `u16` internal
and `u32` external attributes, `u64` local header offset, `u32` comment size, `bool` directory,
encrypted and supported, `i32` `m_is_utf8`, `str` name and comment), `u32`
`mz_zip_reader_get_extra` and `str` what it wrote and `i32` the error, `bool` the entry is 8 MiB or
smaller; for such an entry `mz_zip_reader_extract_to_mem` into a buffer of its size (`bool`, `i32`,
`u64` hash, and the bytes when 300 or fewer), into one byte less (`bool`, `i32`), of its compressed
bytes (`bool`, `i32`, `u64` hash), `mz_zip_reader_extract_to_callback` (`bool`, `i32`, `u64` calls,
the first six calls' `u64` offset and size, `u64` the hash of every call's two, `u64` the bytes'
count and hash) and `mz_zip_reader_extract_to_file` (`bool`, `i32`, `bool` the file exists, `u64`
its size and hash). For each query `i32` `mz_zip_reader_locate_file` and `i32` the error, then
`u64` the buffer's size, `bool` `mz_zip_reader_extract_file_to_mem`, `i32` the error and `u64` the
buffer's hash; `bool` and `i32` of closing twice (`close_zip_reader`, or `mz_zip_reader_end` for
memory and a read function), `u32` the number of files after. 94 archives, each opened the three
ways (282 records; the committed set 78 of them, most from memory, some from a file, a few through
the read function), and the missing path and the folder.

### ZERR

`u64` 35, then for each code from -1 to 33 `i32` the code, `str` `mz_zip_get_error_string` and
`str` `MZ_Archive::get_errorstr`.

### xml.sop1's values

The records of `xml.sop1` (`parity/oracle/probe/xml.cpp`, added 2026-11-08) come from expat
2.5.0 as the dependency build compiles it (`expat.lib`), with a parser made as `bbs_3mf.cpp` makes
one: `XML_ParserCreate(NULL)`, a start element, an end element and a character data handler, the
entity declaration and external entity handlers `NULL`. They write each value in its own width.
`orca-xml`'s `tests/probes.rs` rebuilds every record and compares it whole.

### XPRS

The inputs: `str` label, `u8` the feeding (0 `XML_Parse` chunk by chunk with the last final, as
the model files are fed through miniz's extraction callback; 1 `XML_GetBuffer` and
`XML_ParseBuffer` of the whole document, final, as the configuration files are; 2 `XML_Parse`
chunk by chunk with none final; 3 the same, then `XML_Parse(NULL, 0, 1)`; 4 `XML_GetBuffer` and
`XML_ParseBuffer` chunk by chunk with the last final), `str` the document, `u64` count and `u32`
each size of the chunk pattern (repeated over the document, the last chunk cut; none: one chunk
of it all), `i32` the callback a handler calls `XML_StopParser(parser, XML_FALSE)` at (-1 none),
`bool` it calls it again at every later callback. The feeding stops at the first call that does
not return `XML_STATUS_OK` or after a stop, as the reader's callback throws then.

What expat did: `bool` `XML_GetBuffer` returned `NULL`; `u64` the number of calls and each `u32`
length and `i32` status; `u64` the number of callbacks and each `u8` kind (`S`, `E` or `C`), `str`
the name or the character data, for `S` `i32` `XML_GetSpecifiedAttributeCount` and `u64` count and
`str` each attribute name and value in turn, then `i32` what the stop returned (-1: no stop there);
then `i32` `XML_GetErrorCode`, `bool` and `str` `XML_ErrorString` of it, `u32`
`XML_GetCurrentLineNumber`, `u32` `XML_GetCurrentColumnNumber`, `i32` the parsing status of
`XML_GetParsingStatus` and `i32` `XML_GetSpecifiedAttributeCount`.

The documents: 132 written by hand (the 3MF reader's files, every encoding expat knows,
declared and detected, the internal subset, character references, CDATA sections, line ends, long
names, values and text, deep nesting and the errors), each fed every way and with patterns of 1,
2, 3, 7, 64 and seeded sizes; stops at the first, second, a third, half and last callback; 14
seeded mutations of each; generated model and configuration files, the larger in miniz's 32 and
64 KiB chunks. 7755 records; the committed set 1196 of the small ones.

### XERR

`u64` 46, then for each code from -1 to 44 `i32` the code, `bool` `XML_ErrorString` is not `NULL`
and `str` it.

### model.sop1's values

The records of `model.sop1` (`parity/oracle/probe/model.cpp`, added 2026-11-07) run under the
oracle's threading (one thread in an arena of 8) and write each value in its own width. In them a
matrix is 16 `f64` row by row; a box (`BoundingBoxf3`) is `bool` defined, three `f64` min and three
max; a scaled box (`BoundingBox`) is `bool` defined and `i64` min x, min y, max x and max y; a
polygon is a `u64` count and each point's `i64` x and y; a list of points is a `u64` count and each
point's `f64` x and y; a circle is `f64` centre x, centre y and radius; a configuration dump is a
`str` of `key=value` and a line feed for each option in key order, each value as `opt_serialize`
writes it; settings are a `u64` count and each option's `str` key and `str` value
(`opt_serialize`), which the C++ sets with `set_deserialize_strict`. A bed is the list of points of
its outline, `f64` height, `u64` the number of extruder areas and each area's list of points and
`f64` height. `orca-model`'s `tests/probes.rs` rebuilds every record from the Rust and compares it
whole.

### MXFM

One `Geometry::Transformation`: three `f64` each of an offset, a rotation, a scale and a mirror,
the matrix a 3MF file could give (a rotation, a scale, sometimes mirrored or sheared, and an
offset), `str` a text; then of the transformation set from the four with `set_offset`,
`set_rotation`, `set_scaling_factor` and `set_mirror`: the matrix `get_matrix()`,
`get_matrix_no_offset()`, three `f64` each of `get_offset()`, `get_rotation()`,
`get_scaling_factor()` and `get_mirror()`, `bool` `is_left_handed()`; of `Transformation(matrix)`:
three `f64` each of `get_offset()`, `get_rotation()`, `get_scaling_factor()` and `get_mirror()`,
`bool` `is_left_handed()`, `get_matrix_no_offset()`; and `transform3d_from_string(text)`. 160
cases (11 texts, 6 matrices by hand), the committed set the first 40.

### MFAC

One painting through a fresh part's `FacetsAnnotation`: `u64` the number of triangles, each `i32`
its id and `str` its string (hexadecimal digits, now and then another character), `i32` an id not
in the list; then the data `set_triangle_from_string` of each leaves (`u64` the number of
triangles to split and each `i32` triangle and `i32` start in the bit stream, `u64` the number of
bits and each as `u8`, `u64` the number of used states and each as `u8`), `str`
`get_triangle_as_string` of each triangle and of the absent one, then the data after `reset()`
and the last triangle set again. A string that would make `update_used_states` read past the
stream's last 32-bit word is not recorded (`parity/HAZARDS.md`, orca-model). 60 cases, the
committed set the first 30.

### MCFG

`ModelConfig::set_deserialize` under `EnableSilent`: `str` key, `str` value, then `u8` 0 and the
configuration dump, or `u8` 1 and `str` the exception's `what()`. 34 cases (integers, floats,
percents, booleans, enums, lists, nil, an unknown key, legacy keys), all in the committed set.

### MCIR

The list of points, then `circle_ransac(points, 20, &error)`: the circle and `f64` the error (-1,
as passed, when there are fewer than three points); when there are points,
`smallest_enclosing_circle_welzl` of them scaled (`Polygon::new_scale`): the circle. 80 cases
(random points, round beds of 4 to 153 points rounded to 0.01 or 0.1 or not, ellipses; none, one
and two points), the committed set the first 24.

### MBVL

One `BuildVolume`: the bed; then `i32` `type()`, `str` `type_name()`, the scaled box
`bounding_box()`, the box `bounding_volume()`, the polygons `convex_hull()` and `polygon()`, the
circle `circle()`, the lists of points of the top and bottom chains of the scene's and then the
bed's decomposition, `u64` `get_extruder_area_count()` and for each extruder volume `bool`
`same_with_bed`, `i32` type, the scaled box, the box and the circle. Then 24 cases, each the
matrix, `bool` `may_be_below_bed` and `ignore_bottom`, `u8` `object_state` of a unit cube through
the matrix cast to floats, and for a rectangle `u8` `volume_state_bbox` of the cube's
`transformed_bounding_box`. 33 beds (rectangles, circles, convex and L-shaped outlines, extruder
areas the same as the bed's or not, height 0), the committed set the first 10.

### MLOD

One model file as the command line loads it (`cli/run.cpp`, `load_model`): `str` label, `str` the
file's name, `str` its bytes; the inputs of the steps after loading: two instances of twelve `f64`
(offset, scale, rotation, mirror), `bool` `allow_negative_z`, `u64` the number of beds and each
bed, the settings, three arrange results of `i32` bed, `i64` x and y, `f64` rotation and `i32` item
id. The file is written under its name to a temporary folder and read with
`Model::read_from_file(path, ..., LoadModel | AddDefaultInstances)`: `u8` 1 and `str` the
exception's `what()`, or `u8` 0, each configuration cut to `extruder`, the layer ranges and
profile cleared, `ensure_on_bed()`, and the model: `u64` the number of objects, each `str` name,
`bool` `printable`, three `f64` `origin_translation`, the configuration dump, `u64` the number of
instances and each one's matrix, `u8` `print_volume_state`, `bool` `printable` and `auto_drop` and
`i32` `arrange_order`, `u64` the number of parts and each one's `str` name, `i32` type, matrix,
`u64` vertex and face counts and `u64` the hash of its mesh, `bool` it has a hull and then the
hull's counts and hash, `i32` source object and part, three `f64` source offset, the configuration
dump and `i32` `get_repaired_errors_count()`. A hash is FNV-1a (64 bits) of `u64` the vertex
count, each vertex's three `f32`, `u64` the face count and each face's three `i32`. Then `bool`
every part of the first object has a hull (the command line refuses one without, so the record
ends there), and of that object: the boxes `bounding_box_approx()` and `bounding_box_exact()`,
`f64` `min_z()` and `max_z()`, the boxes `instance_bounding_box(0, false)` and `(0, true)`,
`instance_convex_hull_bounding_box(0, false)` and `(0, true)`, `raw_bounding_box()`,
`raw_mesh_bounding_box()` and `full_raw_mesh_bounding_box()`, `u64` `facets_count()` and
`parts_count()`, `f64` `get_instance_min_z(0)` and `get_instance_max_z(0)`, the polygon of the
instance's `convex_hull_2d()`; the two instances added with `add_instance(offset, scale, rotation,
mirror)`, and for each of the three its matrix, `instance_bounding_box(i, false)`, `f64` its min and
max z and its `convex_hull_2d()`; `bounding_box_exact()`; after `ensure_on_bed(allow_negative_z)`
each instance's three `f64` offset and `f64` `min_z()`; for each bed `u32`
`Model::update_print_volume_state` and each instance's `u8` state; for each instance the polygon
of `get_instance_arrange_poly(instance, settings)` (the contour, `i64` translation x and y, `f64`
rotation, `i32` bed, `u64` the number of filaments and each `i32`, `i32` bed, first-layer bed,
print, first-layer print and vitrification temperatures and the temperature class, `f64` brim,
`bool` tree supports, `f64` height, `str` name, `i64` inflation), then the arrange result set and
`apply()` twice: the matrix, `i32` `arrange_order` and `i32` `is_applied`; `bounding_box_exact()`;
after `center_around_origin(false)` three `f64` `origin_translation`, each part's three `f64`
offset and `bounding_box_exact()`. The files: 18 made by hand (boxes on, off, below, longer and
taller than the bed, two in a file, a triangle, a flat square, a tetrahedron, ASCII with LF and
CRLF, an `.oltp` file, an upper-case extension, empty, no triangles, cut short, `.step`, no
extension), 30 seeded boxes (rotated, some with a second), OrcaSlicer's test STL files but
`20mmbox-CR.stl` and its 21 OBJ meshes as binary STL as Sketch writes `model.stl`
(`parity/CORPUS.md`). The committed set: the files of 4000 bytes or fewer, of the seeded boxes the
first 8.

### MARP

The bed and inflation steps of arrangement: the settings (the bed outline, sometimes extruder
areas, the skirt's loops, height and distance, the draft shield, the line widths and the nozzle
diameters), the parameters `bool` `is_seq_print`, `f32` `bed_shrink_x`, `bed_shrink_y` and
`clearance_radius`, `i64` `min_obj_distance`, `f32` `nozzle_height` and `object_skirt_offset`; `u64`
the number of selected items and each one's polygon, `f64` height, `f64` brim and `bool` tree
supports; `u64` the number of other items and each one's polygon, `f64` brim, `bool`
`is_virt_object` and `bool` `is_extrusion_cali_object`. Then the polygons `get_bed_shape(config)`
and `get_bed_shape(config, false)`; after `update_arrange_params` `f32` `brim_skirt_distance`,
`bed_shrink_x` and `bed_shrink_y`; the polygon `get_shrink_bedpts`; after
`update_selected_items_inflation` `i64` `min_obj_distance` and each selected item's `i64`
inflation; after `update_unselected_items_inflation` each other item's `i64` inflation and its
polygon. 60 cases, the committed set the first 20.

### MTMP

One record: `u64` the number of filament types and each `str` type and `i32`
`Print::get_filament_temp_type`; `u64` the number of sets and each set's `u64` count and `i32`
classes and `i32` `Print::get_compatible_filament_type`.

## Versions

A change to any record, its fields or their order is a new version. The reader refuses a version it
does not know.
