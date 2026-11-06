# Hazards: where the C++ engine's output hangs on more than its inputs

Every place where the C++ engine's bytes depend on something the Rust translation must reproduce
exactly (a runtime library's arithmetic, an order the language leaves open, a thread count, a
clock) or must pin (a random seed, the time, the user), with its source line and its class. Line
numbers are of OrcaSlicer v2.4.2 (commit 8500fcdccaa10b5099ac20d252af3a7c560046f1) with
`patches/` applied, as `<work>/src` holds it; `cli/` lines are this repository's. Listed
2026-11-06 from the source and from the oracle's linker map
(`<work>/oracle/dist/bin/sketch-orca-oracle.map`).

The classes:

| Class | What the translation does |
| --- | --- |
| **R** reproduce | The Rust gives the same bits by construction: a translation of the same code (`orca-stl`, `orca-tbb`, `orca-fmt`), or a call of the same library (`orca-crt` calls the static UCRT). The probe files (`parity/probes/`, `parity/SOP1.md`) hold the C++ behaviour to compare with. |
| **P** pin | The C++ result varies from run to run (a clock, an unseeded generator, the user's name). The oracle pins it with a knob (`parity/oracle/README.md`); the translation takes the same pinned value, and the parity masks (`parity/masks.json`) cover it for the shipped engine. |
| **O** out of reach | Code the front end never reaches (the GUI's, SLA's, debug output, timing logs): listed so that a change in what the front end reaches is noticed. |

Status is what holds the translation to the C++ today: **probed** (a probe record covers it and
the crates' probe tests, `crates/orca-{crt,fmt,stl,tbb}/tests/probes.rs`, replay it: every record
of the committed subset and of the full set gave the same bits, outputs, orders and comparator
calls on 2026-11-06), **pinned** (an oracle knob), **open** (nothing yet; the task that reaches the
code must), **n/a** (class O).

## The C runtime's maths (class R)

The oracle's linker map lists these 53 maths functions of the static UCRT (`libucrt`) and the C
runtime (`LIBCMT`), all **probed** (`crt.sop1` MATH, with FMA3 on and off; MATS for `nan`):

`__vdecl_floor2 _copysign _finite _fpclass _hypot _isnan acos acosf asin atan atan2 atan2f atanf
ceil ceilf copysign cos cosf exp expf fabs floor floorf fma fmax fmaxf fminf fmod fmodf frexp
hypot ldexp llround log log10 logf lrint lround nan nearbyint nextafter nexttoward pow powf round
roundf sin sinf sqrt sqrtf tan tanf truncf`

- `__vdecl_floor2` (`LIBCMT:svml_dfloor2_dispatch.obj`) is SVML's two-lane `floor`: MSVC's
  auto-vectoriser turned a loop of `std::floor` into it. Which loop is not in the map; the probe
  holds its results against the scalar `floor`'s, and a vectorised loop in the translation must
  call it lane for lane (`dd>dd`).
- The UCRT picks FMA3 code paths at start when the processor has them (`_get_FMA3_enable`), and
  their results differ from the SSE2 paths' in the last bit for some functions. The oracle's knob
  `SKETCH_ORCA_ORACLE_FMA3` (patch 0004) pins the choice; the probe records both.
- Helpers the map lists that are not called by the engine's code but by the functions above:
  `_dclass`, `_dsign` (`nan.obj`), `_drint` (`rint.obj`), `_dxp_addh`, `_dxp_addx`, `_dxp_getw`,
  `_dxp_mulh`, `_dxp_setw` (`fma.obj`), `_tan_piby4` (`tan.obj`). Not probed: they are covered
  through their callers.
- `sqrt`, `sqrtf`, `fabs`, `fabsf` and `copysignf` are compiled to instructions (`sqrtsd`,
  `sqrtss`, sign-bit masks) where MSVC can; `orca-crt` does the same, and calls the UCRT where
  MSVC does (a negative `sqrt` argument).

## Float-to-integer conversions (class R)

Every `(int)`, `(coord_t)`, `static_cast<long long>` and the like of a `double` or `float` is
MSVC's `cvttsd2si` / `cvttss2si`, and a conversion to `uint64_t` MSVC's compare-and-subtract
sequence; out-of-range values give the "integer indefinite" value (`0x80000000...`), not Rust's
saturation. **Probed** (`crt.sop1` CAST, every source and target type). The translation converts
only through `orca-crt`'s casts, never with `as`. The sites are too many to list (every
`scale_()`, `scaled<>()`, `coord_t(...)` and `std::round` cast to an integer in libslic3r); the
hot ones:

| Site | What |
| --- | --- |
| `src/libslic3r/libslic3r.h` `scale_` | `coord_t(x / SCALING_FACTOR)` |
| `src/libslic3r/Point.hpp` `scaled`, `Point::new_scale` | the same in templates |
| `src/libslic3r/Slicing.cpp`, `Layer.cpp` | layer heights to scaled integers |
| `src/libslic3r/GCode/GCodeProcessor.cpp` | times and lengths to integers in the statistics |

## `rand()` (class P, and R for its sequence)

The UCRT's `rand` keeps its state per thread; a new thread starts from seed 1. **Probed**
(`crt.sop1` RAND: a fresh thread, after `srand`, and per thread). Every reached site:

| Site | What | Status |
| --- | --- | --- |
| `src/libslic3r/Fill/Lightning/Generator.cpp:34` | `srand(time(NULL))` once per generator, on the thread that builds it | pinned (`SKETCH_ORCA_ORACLE_TIME`, patch 0001, seeds it with the pinned time) |
| `src/libslic3r/Fill/Lightning/Generator.cpp:38` | `rand() % 1000000`, the generator's id | probed; follows the pinned seed |
| `src/libslic3r/Fill/Lightning/TreeNode.cpp:365` | `rand() % m_children.size()`, the first child a tree walks | probed: the thread's sequence, from seed 1 or from line 34's `srand` on that thread |
| `src/libslic3r/Fill/Fill.cpp:201-202` | `rand() / RAND_MAX`, the random and pseudorandom infill joints (cases 12 and 13) | probed, as above |
| `src/libslic3r/GCode/SeamPlacer.cpp:1397` | `float(rand()) / float(RAND_MAX)`, the random seam | probed, as above |
| `src/libslic3r/PrintObject.cpp:3236` | `rand()`, in the vertical shell's random choice | probed, as above |
| `src/libslic3r/GCodeSender.cpp:59`, `:499` | the printer connection's `srand(time)` and `rand()` | n/a (O: no serial printing in the front end) |
| `src/libslic3r/JumpPointSearch.cpp:282`, `PerimeterGenerator.cpp:1075` | debug SVG file names | n/a (O: debug builds only) |

`rand` is not seeded from entropy anywhere the front end reaches but line 34, so with the time
pinned every sequence is fixed; the oracle adds no `srand` (patch 0002). The translation draws
`rand` through `orca-crt`'s per-thread generator, so which thread draws (a TBB worker's or the
caller's, and whether line 34 ran on it before) matters as it does in the C++.

## Random engines, thread-local generators, the time and the user

| Site | Class | What | Status |
| --- | --- | --- | --- |
| `src/libslic3r/Feature/FuzzySkin/FuzzySkin.cpp:26-29` | P | `thread_local std::random_device` seeding a `thread_local std::mt19937` (or the thread id's hash), `uniform_real_distribution<double>(0, 1)`: fuzzy skin | pinned (seed knob); the engine and distribution probed (`stl.sop1` RNGS) |
| `src/libslic3r/GCode.cpp:2707` | P | the placeholder parser's `std::mt19937` seeded from `high_resolution_clock::now()` (`random()` in custom G-code) | pinned (seed knob) |
| `src/libslic3r/PlaceholderParser.cpp:647` | R | `random(a, b)`: `uniform_int_distribution` / `uniform_real_distribution` on that engine | probed (RNGS) |
| `src/libslic3r/ShortEdgeCollapse.cpp:82` | R | `std::mt19937_64` with the constant seed 27644437 | probed (RNGS) |
| `src/libslic3r/TriangleSetSampling.cpp:31` | R | `std::mt19937_64` with the constant seed 27644437 | probed (RNGS) |
| `src/libslic3r/Fill/FillRectilinear.cpp:2221`, `:2869`, `:3849` | R | default-seeded `std::mt19937_64`, `std::shuffle` and distributions in monotonic infill's ant colony | probed (RNGS, SHUF) |
| `src/libslic3r/Geometry/Circle.cpp:116` | R | default-seeded `std::mt19937`, `std::shuffle` (circle fitting) | probed (RNGS, SHUF) |
| `src/libslic3r/Color.cpp:116-120` | O | `std::random_device` for random colours | n/a (GUI) |
| `src/libslic3r/SLAPrintSteps.cpp:77`, `SLA/SupportPointGenerator.cpp:64`, `:334-421`, `.hpp:45`, `:196-230` | O | SLA's generators and ids | n/a (SLA) |
| `src/libslic3r/PlaceholderParser.cpp:78-100`, `:82` | P | `timestamp`, `year` ... `second` placeholders: `localtime` | pinned (`SKETCH_ORCA_ORACLE_TIME`, patch 0001) |
| `src/libslic3r/GCode.cpp:2576` (`Utils::local_timestamp`, `Utils.hpp:666`) | P | the G-code header's "generated by ... on" time | pinned (time knob) |
| `src/libslic3r/Model.cpp:965-966` | P | `std::time` and `localtime` in the backup folder's name (`Model::get_backup_path`) | pinned (time knob) |
| `src/libslic3r/PresetBundle.cpp:1417`, `:1636` | P | presets' imported time | pinned (time knob); not written by the front end |
| `deps_src/miniz/miniz.c` (`mz_zip_writer_add_mem_ex_v2`, `mz_zip_writer_add_read_buf_callback`, `mz_zip_writer_add_staged_open`, and `mz_zip_writer_add_file_ex`'s file time) | P | the time of every entry of a 3MF | pinned (time knob) |
| `src/libslic3r/FilamentGroup.hpp:42-47` (`FlushTimeMachine`) | P | a time budget that ends filament grouping's search: a slow run groups differently | pinned (time knob: no time passes, the search ends by its own count) |
| `src/libslic3r/PlaceholderParser.cpp:105` | P | `USER` / `USERNAME` from the environment, the `user` placeholder | pinned: the parity runs set the environment (`scripts/lib/parity/engineEnv.ts`) |
| `src/libslic3r/GCode/PostProcessor.cpp:159` | O | `SHELL` for post-processing scripts | n/a (the front end runs none) |
| `src/libslic3r/PrintObject.cpp:3722-3728`, `Timer.cpp:6`, `:11`, `Timer.hpp:34`, `Support/TreeModelVolumes.cpp:159-249`, `Support/TreeSupport.cpp:139-153`, `Support/TreeSupport3D.cpp:2472-2521`, `:3423-3608`, `Format/bbs_3mf.cpp:8747-8750` | O | timing for logs; never in an output | n/a |
| `cli/main.cpp:159`, `cli/common.cpp:640` | R | `SKETCH_ORCA_LOG_LEVEL` and the smoke test's crash trigger `SKETCH_ORCA_TEST_CRASH` | the translation reads the same variables (`crates/orca-cli/src/crash.rs`, which also takes `<stage>:panic` and the point `decline`) |

## Orders the language leaves open

### Pointer-keyed unordered containers (class R, order by address)

An `unordered_map` or `unordered_set` keyed by a pointer iterates in an order that depends on the
addresses the allocator gave, which the translation cannot reproduce. Where the iteration order
reaches an output, the translation must find an order-free rewrite or a key that is not an
address, and say so at the site.

| Site | Iterated into an output? | Status |
| --- | --- | --- |
| `src/libslic3r/MinimumSpanningTree.cpp:46-47` | the tree's edges come out in the order of the search; ties broken by iteration | open |
| `src/libslic3r/PerimeterGenerator.cpp:2298` | lookup only | open (check) |
| `src/libslic3r/PrintObject.cpp:3042`, `src/libslic3r/Fill/Fill.cpp:712` | membership of used segments | open (check) |
| `src/libslic3r/GCode/ExtrusionProcessor.hpp:296-299` | lookup by object | open (check) |
| `src/libslic3r/GCode/SeamPlacer.hpp:142` | lookup by object | open (check) |
| `src/libslic3r/GCode/TimelapsePosPicker.hpp:75` | lookup by instance | open (check) |
| `src/libslic3r/PresetBundle.cpp:2549`, `:2605` | O: preset compatibility in the GUI | n/a |

`std::unordered_map` and `std::unordered_set` of values (integers, strings, doubles) iterate in
MSVC's order, which `orca-stl`'s `UnorderedMap` reproduces: **probed** (`stl.sop1` UMAP, USET,
HASH).

### Heaps and priority queues (class R)

Elements that compare equal leave a heap in an order the algorithm decides: **probed** (`stl.sop1`
SORT `heap`, `priority_queue`).

| Site | What |
| --- | --- |
| `src/libslic3r/FilamentGroup.cpp:369`, `FilamentGroup.hpp:65` | filament grouping's queues |
| `src/libslic3r/MultiPoint.cpp:290-319` | Visvalingam simplification's heap |
| `src/libslic3r/Arachne/SkeletalTrapezoidation.cpp:1858` | Arachne's further edges |
| `src/libslic3r/GCode/TimelapsePosPicker.cpp:381` | timelapse candidates |
| `src/libslic3r/GCode/ConflictChecker.hpp:114` | a queue of pointers ordered by a comparator on their buckets |

### Sorts, selections and comparators (class R)

`std::sort` is introsort, `std::stable_sort` a buffered merge sort whose result with a
non-strict-weak comparator depends on the buffer it got, `std::nth_element` and
`std::partial_sort` leave the unsorted part in an order of their own: **probed** (`stl.sop1`
SORT, every comparator call). Comparators that are not strict weak orders (`<=`, a NaN key, an
inconsistent tie-break) make the result depend on the algorithm's exact steps, which `orca-stl`
reproduces call for call; such a comparator is translated as it is, never "fixed". A search of
libslic3r for `<=` and `>=` in sort comparators found none written inline; comparators on
`double` keys that can be NaN (lengths, areas, angles) are the case to watch, and each site is
checked as it is translated.

## Threads (class R)

The oracle runs with one thread inside a `task_arena` of concurrency P
(`SKETCH_ORCA_ORACLE_THREADS=P`, patch 0003), so the split trees are those of P threads while the
work runs in one order. `orca-tbb` reproduces the partitioners' split decisions: **probed**
(`tbb.sop1` TBBF, TBBR, TBBO for P = 1 to 64).

| Site | What | Status |
| --- | --- | --- |
| `src/libslic3r/Support/TreeSupport3D.cpp:2385` | `num_threads = this_task_arena::max_concurrency()` sizes per-thread buffers whose merge order reaches the output | probed (TBBA in each arena; outside any arena oneTBB answers the machine's processor count, which the record holds as the recording machine's, 32) |
| `src/libslic3r/Thread.cpp:222` | `max_concurrency()` for the thread names | n/a (logs) |
| `src/libslic3r/Support/TreeModelVolumes.cpp:112` | a grain size from `max_concurrency()` | probed (TBBA, TBBF) |
| `src/libslic3r/Execution/ExecutionTBB.hpp:60-72` | `execution::reduce` is `tbb::parallel_reduce` (a reduction tree with P), `max_concurrency()` | probed (TBBR) |
| `src/libslic3r/SlicesToTriangleMesh.cpp:107` | `execution::reduce` over layers | probed (TBBR) |
| `src/libslic3r/SLA/Rotfinder.cpp:58`, `:149`, `:94-263`, `SLA/Concurrency.hpp:32-60`, `MarchingSquares.hpp:157`, `MeshSplitImpl.hpp:270`, `SLAPrint.hpp:422`, `CSGMesh/*.hpp` | SLA and CSG | n/a (O) |
| `src/libslic3r/PrintObject.cpp:346`, `:2504`, `src/libslic3r/GCode/ConflictChecker.cpp:272`, `src/libslic3r/Support/TreeSupport.cpp:812`, `:2750`, `:3476`, `TreeSupport.hpp:338-339` | `concurrent_vector`, `concurrent_unordered_set` and `_map` filled from parallel loops: their order is the order of the pushes, one thread's order under the oracle | open: the translation fills them in the same order with one thread, and sorts or keys them where the C++ result is order-free |
| `src/libslic3r/PrintObject.cpp:832`, `Support/SupportMaterial.cpp:353`, `Support/SupportLayer.hpp:140`, `Support/TreeSupport.cpp:3458`, `:3731`, `TreeSupport.hpp:296`, `Format/SL1.cpp:252-259` | `spin_mutex`: the order threads take it is the order of their pushes | open, as above |

## Geometry kernels (class R)

| What | Where | Status |
| --- | --- | --- |
| Eigen 3.5.0 (as its `Version` header numbers it) | `Point.hpp:15-41` and every `Vec`, `Mat`, `Transform3d`; `.norm()`, `.dot()`, `.normalized()` and the other reductions (about 1100 sites in libslic3r) | open: Eigen's evaluation order (its packet reductions sum in a tree of SSE2 lanes, not left to right) is translated with the code that uses it |
| Boost.Polygon 1.84.0's Voronoi builder | `Geometry/Voronoi.cpp:28-29`, `:307`, `Geometry/Voronoi.hpp:100-124`, `Geometry/MedialAxis.cpp:460-468`, `Arachne/SkeletalTrapezoidation.cpp:378`, `MultiMaterialSegmentation.cpp:1555` | open: its predicates use `extended_int<64>` (64 chunks of 32 bits, 2048 bits; `boost/polygon/detail/voronoi_ctypes.hpp:228`) and its own `extended_exponent_fpt`, which the translation carries exactly |
| 128-bit integers | `Int128.hpp:62-244` (`__int128` where the compiler has it, else `_mul128`: MSVC takes the `_mul128` branch), `Point.cpp:252-257`, `deps_src/clipper/clipper.cpp:319` (Clipper's `sign_determinant_2x2_filtered`), `Arrange.cpp:27`, `MinAreaBoundingBox.cpp:28` (`boost::rational<__int128>`, not compiled with MSVC) | open: Rust's `i128` gives the same integers |

## Single precision (class R)

About 2300 lines of libslic3r declare `float`s: extrusion widths and heights
(`ExtrusionEntity.hpp:162` and on), G-code processor times and lengths, seam placer scores,
support tree radii. The translation uses `f32` wherever the C++ does, and MSVC's `/fp:precise`
evaluates `float` arithmetic in `float` (no wider intermediate), which Rust's `f32` arithmetic
matches. Mixed `float` and `double` expressions promote as C++ does and are translated with
explicit casts in the same places. Status: open, site by site as the code is translated.

## Formatting and parsing (class R)

Numbers become text through the UCRT's `printf` family (about 1200 formatting sites in libslic3r
and `cli/`: `sprintf`, `snprintf`, `std::to_string`, `std::ostringstream`, `boost::format`, the
G-code writer's own buffers), through `std::to_chars`, and through nlohmann's `dump`; text
becomes numbers through `strtod`, `atof`, `std::stod`, `std::from_chars`, `boost::lexical_cast`
and `istream >>` (about 600 sites). **Probed** (`fmt.sop1` PRTF, TOCH, OSTR, TOST, PARS);
`orca-fmt` reproduces them. `boost::lexical_cast<double>` and `istream >> double` go through the
STL's `num_get`: `orca-fmt`'s `istream` translates MSVC 14.42's (`xlocnum`: the 768 significant
digits, the sticky last digit, the exponent held to 1100, `_Stolx` for `int`), and `orca-config`
reads every option value through it, but the probe does not hold `num_get` yet, so it is checked
only through the configdef probe's round trips: open. `sscanf("%lf")` (a point's `x` and `y`) is
modelled as the UCRT's `strtod` of the same characters: open, unprobed. nlohmann's number output
is its own Grisu2 (`deps_src/nlohmann/json.hpp`), translated in `orca-json` (`src/dtoa.rs`) and
held to the front end's goldens: done. The configdef probe records each default as its
serialisation (six significant digits for a double), so `OptionDef::default_value` holds that
text read back, not the C++ default's full double; a default whose digits matter past the sixth
is open until the probe records the double's bits.

## File-system order (class R or O)

`directory_iterator` returns entries in the order the file system keeps them (NTFS: by name, case
folded; other file systems: no order).

| Site | What | Status |
| --- | --- | --- |
| `cli/presets.cpp:679` | the front end lists `resources/profiles` | open: the translation sorts as NTFS does, and the C++ result is order-free where it loads every vendor |
| `src/libslic3r/PresetBundle.cpp:268`, `:480`, `:962`, `:998`, `:2205`, `:2322`, `:2357`, `src/libslic3r/Preset.cpp:1608`, `:4120` | preset folders | n/a (O: the front end loads presets by file, not by folder) |
| `src/libslic3r/Format/bbs_3mf.cpp:8541` | a 3MF's temporary folder | open (check) |
| `src/libslic3r/utils.cpp:950`, `:1633`, `src/libslic3r/CutSurface.cpp:3629`, `src/libslic3r/Emboss.cpp:1011` | copies, removals, fonts | n/a (O) |

## What the probe gate and the first record found (2026-11-06)

- `__vdecl_floor2` gives each lane's UCRT `floor` on every recorded case of both sets, so
  `orca-crt`'s `floor2` calls `floor` per lane; no `asm!` shim into SVML is needed.
- No symbol of Rust's `compiler-builtins` shadows the UCRT's: every MATH record, FMA3 on and off,
  matches through `orca-crt`'s `#![no_builtins]` wrappers.
- TBBA `outside` is the recording machine's processor count (see "Threads"); the probe test
  compares it only on a machine with as many processors.
- `parity record` found two outputs of class R (`scripts/lib/parity/classes.ts`): `out.gcode` and
  `result.json` of `family-infill-lightning` and `family-fuzzy-skin` differ between the shipped
  engine's own runs. They are the `srand(time(NULL))` of `Fill/Lightning/Generator.cpp:34` and
  fuzzy skin's `random_device` (`Feature/FuzzySkin/FuzzySkin.cpp:26-29`): pinned in the oracle,
  but `parity/masks.json` does not cover them for the shipped engine, so the translation of those
  families is compared with the oracle's pinned output only.

## Keeping this list

A new hazard found while translating goes here with its line and class, in the change that
translates it. A new CRT maths function in the oracle's map makes `parity probe` fail until the
probe calls it (`parity/oracle/probe/crt.cpp`).
