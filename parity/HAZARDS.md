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

Status is what holds the translation to the C++ today: **probed** (a probe record covers it and the
crates' probe tests, `crates/orca-{crt,fmt,stl,tbb}/tests/probes.rs`, replay it: every record of the
committed subset and of the full set gave the same bits, outputs, orders and comparator calls on
2026-11-06; phase O2's crates replay their own files from 2026-11-07), **pinned** (an oracle knob),
**open** (nothing yet; the task that reaches the code must), **n/a** (class O).

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
| `cli/main.cpp:159`, `cli/common.cpp:640` | R | `SKETCH_ORCA_LOG_LEVEL` and the smoke test's crash trigger `SKETCH_ORCA_TEST_CRASH` | the translation reads the same variables (`crates/orca-cli/src/log.rs`, which reads the level as `strtoul` does, and `crates/orca-cli/src/crash.rs`, which also takes `<stage>:panic` and the point `decline`) |

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
| Eigen 3.5.0 (as its `Version` header numbers it) | `Point.hpp:15-41` and every `Vec`, `Mat`, `Transform3d`; `.norm()`, `.dot()`, `.normalized()` and the other reductions (about 1100 sites in libslic3r) | probed for the operations phase O2 reaches (orca-eigen, `geom.sop1`'s EVEC, EMAT, ETRF, EROT and ESVD, 2026-11-07; "orca-eigen" below); open for the rest, translated with the code that uses it: Eigen's packet reductions sum in a tree of SSE2 lanes, not left to right |
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

Numbers become text through the UCRT's `printf` family (about 1200 formatting sites in libslic3r and
`cli/`: `sprintf`, `snprintf`, `std::to_string`, `std::ostringstream`, `boost::format`, the G-code
writer's own buffers), through `std::to_chars`, and through nlohmann's `dump`; text becomes numbers
through `strtod`, `atof`, `std::stod`, `std::from_chars`, `boost::lexical_cast` and `istream >>`
(about 600 sites). **Probed** (`fmt.sop1` PRTF, TOCH, OSTR, TOST, PARS); `orca-fmt` reproduces them.
`boost::lexical_cast<double>` and `istream >> double` go through the STL's `num_get`: `orca-fmt`'s
`istream` translates MSVC 14.42's (`xlocnum`: the 768 significant digits, the sticky last digit, the
exponent held to 1100, `_Stolx` for `int`), and `orca-config` reads every option value through it.
`ptree.sop1` PTGT (2026-11-10, orca-ptree-probe) holds `num_get` for `long` (as `get<int>`),
`unsigned long long`, `float` and `double`, read as `property_tree`'s `stream_translator` reads
(`>> value`, then `>> std::ws`, then the end of the data): 273 texts in 15 groups under all four
types (among them the 768-digit ties with and without a digit past them, exponents held to 1100,
fields longer than the 792-character buffer, and `DBL_MAX` and the rounding boundary past it in
full, 2026-11-11, orca-ptree-probe-2), 2000 seeded ones in the full set, and the 3MF reader's own
calls. **Probed**: `orca-xml`'s `tests/ptree.rs` replays every record through `orca-fmt`'s `istream`
(2026-11-11, orca-ptree-port: 60 PTGT committed, 82 in the full set, no difference; replayed again
by orca-xml-stack, with `int` read through the `_Parse_int_with_locale` that `size_t` uses and then
`_Stolx`, and with the six new texts). `sscanf("%lf")` (a point's `x` and `y`) is **probed**
(`cstdio.sop1` CSCN, 2026-11-07), and `orca-fmt`'s `scanf` reproduces it: `%lf` stores `strtod` of
the characters it reads, but it reads them by a grammar of its own, and where that grammar stops
inside a number (`1e`, `1e+`, `0x`, `infin`, `nan(ab`) it stores nothing (a matching failure, or
`EOF` at the end of the input) where `strtod` takes the number before. `orca-config`'s point reader
(`src/value.rs`, `scan_point`) still calls `strtod` on the rest of the text, so on those inputs it
stores a coordinate the C++ leaves as it was and counts its conversions otherwise; calling
`orca_fmt::scanf::sscanf` with the C++ formats would close it: open, for `orca-config`. nlohmann's
number output is its own Grisu2 (`deps_src/nlohmann/json.hpp`), translated in `orca-json`
(`src/dtoa.rs`) and held to the front end's goldens: done. The configdef probe records each default
as its serialisation (six significant digits for a double), so `OptionDef::default_value` holds that
text read back, not the C++ default's full double. A scan of the literals in `PrintConfig.cpp`'s 805
`set_default_value` statements (2026-11-11, orca-info-place) found three whose double is not their
six-digit text, all `float` literals widened to a double: `0.3f` (`fuzzy_skin_point_distance`,
`:3579`), `0.01f` (`machine_max_junction_deviation`, `:4578`) and `0.025f`
(`wall_maximum_deviation`, `:7263`). The front-objects dump's CONF shows them, so orca-cli keeps the
three (`crates/orca-cli/src/config.rs`, `FLOAT_LITERAL_DEFAULTS`); `OptionDef::default_value` and
orca-model's `config.rs` still give the text read back, which is open for them until the probe
records the double's bits.

## File-system order (class R or O)

`directory_iterator` returns entries in the order the file system keeps them (NTFS: by name, case
folded; other file systems: no order).

| Site | What | Status |
| --- | --- | --- |
| `cli/presets.cpp:679` | the front end lists `resources/profiles` | open: the translation sorts as NTFS does, and the C++ result is order-free where it loads every vendor |
| `src/libslic3r/PresetBundle.cpp:268`, `:480`, `:962`, `:998`, `:2205`, `:2322`, `:2357`, `src/libslic3r/Preset.cpp:1608`, `:4120` | preset folders | n/a (O: the front end loads presets by file, not by folder) |
| `src/libslic3r/Format/bbs_3mf.cpp:8541` | a 3MF's temporary folder | open (check) |
| `src/libslic3r/utils.cpp:950`, `:1633`, `src/libslic3r/CutSurface.cpp:3629`, `src/libslic3r/Emboss.cpp:1011` | copies, removals, fonts | n/a (O) |

## O2 sources (2026-11-07)

What phase O2's code hangs on, found by reading its sources (o2-geom's foundation audit): Clipper
6.4.2 (`deps_src/clipper`); libslic3r's `ClipperUtils`, `Point`, `Polygon`, `Polyline`,
`MultiPoint`, `Line`, `ExPolygon`, `BoundingBox`, `Geometry` and `Geometry/ConvexHull`; Qhull's
`libqhull_r` and `libqhullcpp`; admesh and `TriangleMesh`; libnest2d, `Arrange.cpp` and
`ModelArrange.cpp`; Boost 1.84.0's R*-tree, `rational` and `multiprecision`; NLopt 2.5.0
(`<work>/deps/dep_NLopt-prefix/src/dep_NLopt/src`, "NLopt src/" below); `Model`, `BuildVolume`,
the STL, OBJ and 3MF readers, `miniz_extension.cpp` and miniz; expat 2.5.0
(`src/deps/EXPAT/expat`); `PlaceholderParser.cpp`; `cli/run.cpp:250-1068` and `cli/presets.cpp`.
Every line was searched for sorts, heaps, selections, searches, `qsort`, float `min` and `max`,
ordered and unordered containers, TBB, clocks, generators, maths calls and float-to-integer
conversions, and each hit was read where it stands. "Covered by" names what the translation calls
there. What the foundations lacked was `qsort`: `orca-crt` now calls the static UCRT's own
(`crates/orca-crt/src/search.rs`), **probed** (`crt.sop1` QSRT). Nothing else was missing: the
sorts, heaps, selections, searches and generators below are `orca-stl`'s, the loops `orca-tbb`'s,
the maths and conversions `orca-crt`'s.

### Orders

| Site | What decides the order | Class | Covered by |
| --- | --- | --- | --- |
| `deps_src/clipper/clipper.cpp:975` | `std::sort` of the local minima by `Y`, taken from the back (`PopLocalMinima`): equal `Y`s come out as the sort left them | R | `orca_stl::sort_by` |
| `clipper.cpp:1062`, `clipper.hpp:467` | the scanbeam, a `std::priority_queue<cInt>`; popping drops repeats, so only the values count | R | `orca_stl::PriorityQueue` |
| `clipper.cpp:2458-2509` | `BuildIntersectList`: a bubble sort of the active edges by `Curr.x` that records each swap as an intersection | R | translated as written |
| `clipper.cpp:2517`, `:2511-2533` | `std::sort` of the intersections by `Pt.y`, descending (equal `y`s as the sort leaves them), then `FixupIntersectionOrder`'s swaps | R | `orca_stl::sort_by` |
| `clipper.cpp:2652` | `std::sort(m_Maxima)`, `cInt`s | R | `orca_stl::sort` |
| `src/libslic3r/Point.cpp:133`, `:142` | `std::sort` of points, x then y | R | `orca_stl::sort` |
| `Polygon.cpp:505`, `Polyline.cpp:540`, `libslic3r.h:220-221` | `std::unique`, after a sort in `sort_remove_duplicates` | R | `orca_stl::unique_vec`, `orca_stl::sort` |
| `libslic3r.h:261` | `binary_find`: `std::lower_bound` | R | `orca_stl::lower_bound_by` |
| `MultiPoint.cpp:290-319` | Visvalingam's heap of areas, which tie: `make_heap`, `pop_heap` | R | `orca_stl::make_heap_by`, `pop_heap_by` |
| `Geometry.cpp:136` | `std::sort` of `arrange`'s grid cells by squared distance, then position | R | `orca_stl::sort` |
| `Geometry/ConvexHull.cpp:13-14`, `:44` | `convex_hull`: `std::sort` by x then y and `std::unique`; for `Vec3d` on `double` keys | R | `orca_stl::sort_by`, `unique_vec_by` |
| `Geometry/ConvexHull.cpp:342-343`, `:384-385` | `min_element` and `max_element` with a lexicographic comparator, `lower_bound` in the hull's chains | R | `orca_stl::min_element_by`, `max_element_by`, `lower_bound_by` |
| `ClipperUtils.cpp:1143` | `std::max_element` of `double` deltas | R | `orca_stl::max_element_f64` |
| `ClipperUtils.cpp:1186`, `Point.cpp:344` | `std::clamp` of a `double` | R | `orca_stl::clamp` |
| `MultiPoint.cpp:305`, `:388`, `:410`, `:418`, `:555-558`, `Polygon.cpp:550`, `BoundingBox.cpp:243`, `:279-280` | `std::min`, `std::max` of `double`s | R | `orca_stl::min`, `orca_stl::max` |
| `clipper.cpp:521-522`, `:1015-1029`, `:2873-2879`, `:3762`, `Line.cpp:84-89` | `std::min`, `std::max` of integers | R | the C++ expression (`a < b ? b : a`) |
| `deps_src/qhull/src/libqhull_r/mem_r.c:362` | `qsort` of the memory sizes, `qh_intcompare` (a difference) | R | `orca_crt::qsort` (QSRT comparator 0) |
| `merge_r.c:809` | `qsort` of vertices by `visitid` (`qh_comparevisit`, an unsigned difference) | R | `orca_crt::qsort` |
| `merge_r.c:1184`, `:1186`, `:1249`, `:1251` | `qsort` of the merge set: `qh_compareangle` never returns 0, so equal angles land where `qsort`'s calls put them; `qh_comparemerge` a difference | R | `orca_crt::qsort` (QSRT comparators 1 and 0) |
| `io_r.c:494`, `:904`, `:915`, `:3347`, `:3447` | `qsort` for output and for `PFn`, `PAn` (`qh_compare_facetvisit`, `qh_compare_facetarea`, `qh_compare_facetmerge`) | O (TriangleMesh runs `Qt`, prints nothing) | `orca_crt::qsort` if reached |
| `deps_src/admesh/connect.cpp:170` | `std::upper_bound` in the primes table | R | `orca_stl::upper_bound` |
| `TriangleMesh.cpp:558` | `std::sort` of edge pairs | R | `orca_stl::sort` |
| `TriangleMesh.cpp:680` | `std::sort` of vertex indices by x, y, z, then index (total) | R | `orca_stl::sort_by` |
| `TriangleMesh.cpp:807` | `std::map<size_t, size_t>` | R | `BTreeMap` (integer keys, the same order) |
| `deps_src/libnest2d/include/libnest2d/geometry_traits.hpp:1070` | `std::sort` of points, x then y | R | `orca_stl::sort_by` |
| `geometry_traits_nfp.hpp:285` | `std::sort` of edges by angle (`double` keys, ties) | R | `orca_stl::sort_by` |
| `geometry_traits_nfp.hpp:586`, `:588`, `:713` | `std::stable_sort` and `std::sort` of edge references by slope | R | `orca_stl::stable_sort_by`, `orca_stl::sort_by` |
| `selections/djd_heuristic.hpp:135`, `filler.hpp:64` | `std::sort` of items by area, descending: equal areas tie | R | `orca_stl::sort_by` |
| `selections/firstfit.hpp:85` | `std::sort` of items by priority, bed temperature, height, area: equal items tie | R | `orca_stl::sort_by` |
| `placers/nfpplacer.hpp:254`, `utils/rotcalipers.hpp:215` | `std::lower_bound` | R | `orca_stl::lower_bound_by` |
| `placers/nfpplacer.hpp:886`, `:933` | `std::min_element` of the starting points' scores: the first of equals | R | `orca_stl::min_element_by` |
| `src/libslic3r/Arrange.cpp:78`, `:530-537`, `:741-753` | Boost's `rtree<..., rstar<16, 4>>`: its inserts split and reinsert by `std::sort` (`rstar/redistribute_elements.hpp:144`), `std::nth_element` (`:385`, `rstar/choose_next_node.hpp:143`) and `std::partial_sort` (`rstar/insert.hpp:140`), and the `intersects` query returns values in tree order, which the objective function then sums over | R | `orca_stl::sort_by`, `nth_element_by`, `partial_sort_by`; the R*-tree translated as written (orca-arrange) |
| `Arrange.cpp:663`, `:675`, `ModelArrange.cpp:154`, `nfpplacer.hpp:1118` | `std::set<int>` | R | `BTreeSet` |
| NLopt src/`algs/neldermead/sbplx.c:119` | `nlopt_qsort_r` of the subspace indices by `abs(dx)` (ties give 0): NLopt's own quicksort (`util/qsort_r.c:68-104`; `HAVE_QSORT_R_damn_it_use_my_own` is defined nowhere, so it never calls the CRT's `qsort_s` or `qsort_r`) | R | translated with NLopt (orca-arrange), not `orca_crt::qsort` |
| NLopt src/`algs/neldermead/nldrmd.c:38-42`, `:128`, `:262` | the simplex's red-black tree (`util/redblack.c`) keyed by function value, ties broken by the key's address, `k1 - k2`: the vertex's place in the simplex array | R | translated with NLopt: ties by index |
| `src/libslic3r/Model.cpp:1370`, `:1375` | `std::stable_sort` of volumes by type | R | `orca_stl::stable_sort_by` |
| `Model.hpp:1229`, `Model.cpp:3418`, `Format/bbs_3mf.cpp:559-560` | `std::sort` by id, `std::unique` | R | `orca_stl::sort_by`, `unique_vec` |
| `Model.cpp:3549` | `std::lower_bound` in a `TriangleSelector`'s triangles | R | `orca_stl::lower_bound_by` |
| `Format/bbs_3mf.cpp:918-949`, `:1139`, `:1149`, `:2044-2045`, `:2645`, `:2677`, `:6812`, `Model.hpp:121-123`, `:1505`, `:1534`, `:1563`, `:1731`, `PlaceholderParser.cpp:836`, `:1856` | `std::map` keyed by `int`, ids or `std::string` | R | `BTreeMap` (`String` orders by bytes, as `std::string` does) |
| `Format/bbs_3mf.cpp:5819`, `:5832`, `:5847-5848` | the 3MF writer's `std::map`s keyed by pointers; `objects_data` is iterated at `:7794` into `model_settings.config`, in address order | R, order by address | open (o2-model): an order that is not an address, said at the site |
| `cli/run.cpp:599`, `:601` | `std::max_element` of `double`s | R | `orca_stl::max_element_f64` |
| `cli/run.cpp:790` | `std::stable_sort` of corners by squared distance | R | `orca_stl::stable_sort_by` |
| `cli/presets.cpp:682` | `std::sort` of vendor names | R | `orca_stl::sort` |

No container in these sources is keyed by a float, and none of them iterates an unordered
container: `Point.hpp:631`'s `unordered_multimap` (`ClosestPointInRadiusLookup`) serves only code
that phase O2 does not reach (infill, supports, the slicer).

### Threads

| Site | What | Class | Covered by |
| --- | --- | --- | --- |
| `deps_src/libnest2d/include/libnest2d/parallel.hpp:33` (`LIBNEST2D_THREADING_tbb`, `deps_src/libnest2d/CMakeLists.txt:28`), called at `placers/nfpplacer.hpp:601`, `:622`, `:855`, `:905` | `tbb::parallel_for<size_t>(0, N, ...)`; each body writes its own slot (`nfps[n]`, `results[n]`), read after the loop | R, order-free | `orca_tbb::parallel_for_index` |
| `src/libslic3r/Format/bbs_3mf.cpp:1788-1800` | the 3MF reader's object importers, results combined with `&=` under a mutex | R, order-free | `orca_tbb::parallel_for` |
| `Format/bbs_3mf.cpp:7021`, `:8419` | the 3MF writer's objects and plates | R, order-free (each writes its own entry) | `orca_tbb::parallel_for` |
| `clipper.hpp:119`, `Point.hpp:61` | `tbb::scalable_allocator` for paths and points | O (memory only) | `Vec` |

### Clocks and generators

| Site | What | Class | Status |
| --- | --- | --- | --- |
| `PlaceholderParser.cpp:78-100`, `Model.cpp:965-966`, miniz's writer | the time in placeholders, the backup folder's name, 3MF entries | P | pinned (`SKETCH_ORCA_ORACLE_TIME`), as listed in "Random engines, thread-local generators, the time and the user" |
| `PlaceholderParser.cpp:647-657`, `PlaceholderParser.hpp:23` | `random(a, b)` on a `std::mt19937`, default-constructed (seed 5489) for the presets' conditions; `GCode.cpp:2707` seeds it from the clock only for G-code | R | `orca_stl::Mt19937`, `UniformIntDistribution`, `UniformRealDistribution` (probed, RNGS) |
| `deps_src/qhull/src/libqhull_r/global_r.c:1864-1866` | `time()` seeds `qh_rand` to draw `qh->run_id`, which only `FO` prints | O | the translation sets `run_id` without the clock |
| `global_r.c:1589-1615` | reseeds `qh_rand` with `ROTATErandom` (1 unless `QRn`) and draws 1000 numbers to check the generator | R | Qhull's `qh_rand` (`random_r.c:148`, Park and Miller, `qh_RANDOMtype` 5) translated with Qhull |
| `geom_r.c:73`, `:651`, `poly2_r.c:1905`, `libqhull_r.c:712`, `geom2_r.c:912-922` | `qh_rand` draws for `Rn`, `Qr` and `QJ` | O (`TriangleMesh.cpp:1356`, `:1363` run `Qt`) | n/a |
| `user_r.h:219` (`qh_CPUclock`, `clock()`), `libqhull_r.c:446-447`, `:468-469`, `merge_r.c:3467-3468` | timing statistics and trace output | O | n/a |
| `rboxlib_r.c` | rbox, which the engine never calls | O | n/a |
| NLopt src/`api/optimize.c:414`, `api/general.c:121-136`, `util/timer.c:78-91` | `nlopt_srand_time_default`: a thread's first optimisation seeds NLopt's `mt19937ar` from `time(NULL)` (no `gettimeofday`: `nlopt_config.h`) and the thread id; subplex, the one algorithm arrange runs (`nfpplacer.hpp:390`, `:650`), draws nothing from it | O | n/a |
| NLopt src/`api/optimize.c:433`, `util/stop.c:90` | `stop.start = nlopt_seconds()`; compared only when `maxtime > 0`, which libnest2d never sets (`optimizers/nlopt/nlopt_boilerplate.hpp:151-159`) | O | n/a |
| `libnest2d/utils/rotfinder.hpp:15`, `optimizers/nlopt/genetic.hpp:18` | ESCH, a genetic search that draws random numbers | O (`Arrange.cpp:945-1005` rotates by rotating calipers instead) | n/a |
| `src/deps/EXPAT/expat/xmlparse.c:832-847`, `:873` | the hash salt from `rand_s`, else `gettimeofday` | O: the salt moves names between buckets of lookup tables; what the parser reports does not change | the translation uses a fixed salt |
| `deps_src/libnest2d/tools/benchmark.h:27` | a benchmark tool, not compiled | O | n/a |

### Maths calls

Each call is the static UCRT's function of the same name through `orca-crt` (class R; `sqrt`,
`sqrtf` and `fabs` are the IEEE operations there, as MSVC compiles them): **probed** for every
function the oracle's linker map lists (`crt.sop1` MATH).

| File | Function and lines |
| --- | --- |
| `deps_src/clipper/clipper.cpp` | `acos` 3552; `atan2` 3739, 3760; `cos` 3556; `fabs` 392, 402, 409, 509, 512, 516, 519, 3548-3549, 3552-3554, 3697, 3762; `floor` 92; `sin` 3555; `sqrt` 3352; `tan` 3739 |
| `src/libslic3r/ClipperUtils.cpp` | `atan2` 1204; `tan` 1204 |
| `src/libslic3r/Point.cpp` | `atan2` 84-85; `cos` 54; `sin` 53 |
| `src/libslic3r/Point.hpp` | `atan2` 132; `cos` 236, 340; `round` 197, 199, 240-241, 280, 344-345; `sin` 236, 340 |
| `src/libslic3r/Polygon.cpp` | `acos` 745; `ceil` 746; `cos` 287, 757; `floor` 345; `nextafter` 287; `sin` 757 |
| `src/libslic3r/Polygon.hpp` | `cos` 164; `sin` 165 |
| `src/libslic3r/Polyline.cpp` | `sqrt` 67, 71, 698, 702 |
| `src/libslic3r/MultiPoint.cpp` | `cos` 40, 544, 592; `round` 32-33, 43-44, 548-549, 553-554, 583-584, 595-596; `sin` 39, 543, 591; `sqrt` 410, 418 |
| `src/libslic3r/MultiPoint.hpp` | `cos` 31, 132; `sin` 31, 132 |
| `src/libslic3r/Line.cpp` | `fabs` 29, 63 |
| `src/libslic3r/Line.hpp` | `atan2` 176; `fabs` 130, 133; `sqrt` 82, 120, 188, 193 |
| `src/libslic3r/ExPolygon.cpp` | `fmaxf` 281 |
| `src/libslic3r/Geometry.cpp` | `fabs` 31, 33, 38, 40, 209-210, 217; `floor` 104-105, 181-182; `pow` 632; `sqrt` 291 |
| `src/libslic3r/Geometry.hpp` | `fmod` 536; `round` 322 |
| `src/libslic3r/libslic3r.h` | `fabs` 296, 309; `floor` 419 |
| `deps_src/qhull/src/libqhull_r/geom2_r.c` | `sqrt` 203, 231-232, 244, 380, 772, 1294, 1308, 1331 |
| `deps_src/qhull/src/libqhull_r/geom_r.c` | `sqrt` 814, 816, 818, 825, 853 |
| `deps_src/qhull/src/libqhull_r/global_r.c` | `fabs` 1923 |
| `deps_src/qhull/src/libqhull_r/io_r.c` | `fabs` 2784-2786; `sqrt` 856, 1428 |
| `deps_src/qhull/src/libqhull_r/libqhull_r.c` | `floor` 714 |
| `deps_src/qhull/src/libqhull_r/poly2_r.c` | `floor` 1907; `sqrt` 2212, 2283 |
| `deps_src/qhull/src/libqhull_r/rboxlib_r.c` | `asin` 294; `cos` 295, 521, 549, 552, 578, 656; `floor` 273; `ldexp` 273; `pow` 473; `sin` 522, 551, 579, 657; `sqrt` 232, 528, 535, 576, 615, 675, 691 |
| `deps_src/qhull/src/libqhull_r/stat_r.c` | `sqrt` 671 |
| `deps_src/qhull/src/libqhullcpp/QhullHyperplane.cpp` | `fabs` 71, 77; `sqrt` 150 |
| `deps_src/qhull/src/libqhullcpp/QhullPoint.cpp` | `sqrt` 109, 161 |
| `deps_src/qhull/src/libqhullcpp/functionObjects.h` | `fabs` 35 |
| `deps_src/admesh/util.cpp` | `cos` 129, 141, 153, 165, 174, 183; `sin` 130, 142, 154, 166, 175, 184 |
| `src/libslic3r/TriangleMesh.cpp` | `acos` 1238-1239; `ceil` 923, 965, 1018-1019, 1114-1115; `cos` 1031, 1035, 1076, 1125, 1133, 1180, 1188; `sin` 1032, 1036, 1076, 1126, 1133, 1181, 1183, 1188, 1275; `sqrtf` 1277 |
| `deps_src/libnest2d/include/libnest2d/common.hpp` | `cos` 132, 138, 140; `sin` 130, 133, 141 |
| `deps_src/libnest2d/include/libnest2d/geometry_traits.hpp` | `atan2` 461; `sqrt` 344 |
| `deps_src/libnest2d/include/libnest2d/nester.hpp` | `ceil` 863 |
| `deps_src/libnest2d/include/libnest2d/placers/nfpplacer.hpp` | `cos` 264; `floor` 654; `fmod` 245; `pow` 207, 712; `round` 207, 264-265, 396-397, 418-419, 982, 984; `sin` 265; `sqrt` 166, 461 |
| `deps_src/libnest2d/include/libnest2d/selections/djd_heuristic.hpp` | `ceil` 576 |
| `deps_src/libnest2d/include/libnest2d/utils/rotcalipers.hpp` | `atan2` 40; `sqrt` 28, 32 |
| `deps_src/libnest2d/include/libnest2d/utils/rotfinder.hpp` | `sqrt` 22 |
| `deps_src/libnest2d/tools/svgtools.hpp` | `round` 61 |
| `src/libslic3r/Arrange.cpp` | `atan2` 246; `sqrt` 553, 703, 1027 |
| `NLopt src/algs/neldermead/sbplx.c` | `copysign` 231; `fabs` 41, 122, 128, 132, 146, 200-201, 220-221 |
| `NLopt src/algs/neldermead/nldrmd.c` | `fabs` 49, 141, 144, 147, 150, 183, 209, 216 |
| `NLopt src/api/optimize.c` | `fabs` 119-120; `pow` 466 |
| `NLopt src/util/stop.c` | `fabs` 36, 168, 179, 199 |
| `NLopt src/util/mt19937ar.c` | `log` 230; `sqrt` 230 |
| `src/libslic3r/Model.cpp` | `acos` 3143, 3146; `fabs` 2465; `sqrt` 3292 |
| `src/libslic3r/Format/bbs_3mf.cpp` | `fabs` 9265, 9268 |
| `src/libslic3r/PlaceholderParser.cpp` | `ceil` 404, 410, 2077; `floor` 389, 395, 2075; `fmod` 524; `round` 275, 371, 380, 2073 |
| `cli/run.cpp` | `llround` 758; `sqrt` 609, 615 |
| `cli/presets.cpp` | `fabs` 355 |

`rboxlib_r.c` is rbox (class O). `ExPolygon.cpp:281` is the medial axis's, which phase O2 leaves
out.

### Float-to-integer conversions

Each is MSVC 14.42's conversion through `orca-crt`'s `convert` (class R, **probed**, `crt.sop1`
CAST): `f64_to_i64` for `coord_t`, `cInt` and `int64_t`, `f64_to_i32` for `int`, `f64_to_u64` for
`size_t`, and the same of each coefficient for an Eigen `.cast<coord_t>()`. Clipper's `Round<T>`
(`clipper.cpp:95-106`) truncates `FRound(v)` (`:88-93`: `floor(v + 0.5)`, except that
0.49999999999999994 gives 0); `round`, `floor` and `ceil` before a conversion are the UCRT's
(above). The lines found:

| File | Lines |
| --- | --- |
| `deps_src/clipper/clipper.cpp` | 353, 376, 384, 391, 393-394, 3579-3580, 3592-3593, 3646, 3648, 3673, 3675, 3703-3704, 3714-3715, 3717-3718, 3742-3743, 3745-3746, 3753-3754, 3762, 3768-3769, 3775-3776 |
| `src/libslic3r/Point.cpp` | 125 |
| `src/libslic3r/Point.hpp` | 193-197, 199, 204, 206, 240-241, 278-280, 289, 292, 295, 344-345, 423, 429, 483 |
| `src/libslic3r/Polygon.cpp` | 345, 473, 746 |
| `src/libslic3r/Polyline.cpp` | 67, 107, 139, 182, 698 |
| `src/libslic3r/MultiPoint.cpp` | 32-33, 43-44, 548-549, 553-554, 583-584, 595-596 |
| `src/libslic3r/MultiPoint.hpp` | 29, 129 |
| `src/libslic3r/Line.cpp` | 39, 115-116, 123 |
| `src/libslic3r/ExPolygon.cpp` | 303, 313 |
| `src/libslic3r/ExPolygon.hpp` | 41 |
| `src/libslic3r/Geometry.cpp` | 104-105, 865-868 |
| `src/libslic3r/Geometry.hpp` | 134-136, 322 |
| `deps_src/qhull/src/libqhull_r/libqhull_r.c` | 714 |
| `deps_src/qhull/src/libqhull_r/poly2_r.c` | 1907 |
| `deps_src/qhull/src/libqhull_r/rboxlib_r.c` | 273 |
| `src/libslic3r/TriangleMesh.cpp` | 923, 965, 1018-1019, 1114-1115 |
| `deps_src/libnest2d/include/libnest2d/placers/nfpplacer.hpp` | 654 |
| `deps_src/libnest2d/include/libnest2d/selections/djd_heuristic.hpp` | 576 |
| `src/libslic3r/Model.cpp` | 2638 |
| `src/libslic3r/PlaceholderParser.cpp` | 275, 380, 395, 410 |
| `cli/run.cpp` | 1019 |

A conversion the C++ writes without a cast (a `double` assigned to a `coord_t`) is not in this
table; the task that translates the function finds it there.

### Exact integer arithmetic

| Site | What | Covered by |
| --- | --- | --- |
| `deps_src/clipper/clipper.cpp:319` and `src/libslic3r/Int128.hpp:62-244` | Clipper's `sign_determinant_2x2_filtered` and `Int128::multiply`: MSVC takes `_mul128` | probed: `i128` (orca-geom, `geom.sop1` CINT and CCLP) |
| `src/libslic3r/Point.cpp:252-257` | 128-bit products | `i128` (orca-geom) |
| `Geometry/ConvexHull.cpp`'s `rotcalip::cmp_angles` | `boost::multiprecision::int128_t` and `int256_t` products of `int64_t` dot products | probed (GXFM): exact in orca-geom, a sign and a 192-bit magnitude; the `int64_t` dot products wrap first, as in C++ (orca-geom below) |
| `Arrange.cpp:29`, `MinAreaBoundingBox.cpp` | `boost::multiprecision::int128_t` and `boost::rational<int128_t>` | orca-exact (o2-arrange) |

## Phase O2's crates (2026-11-07)

Phase O2 reads models, places them and answers info jobs. Each of its ten crates has a section
here: what it translates, the probe file that holds its C++ behaviour (`parity/SOP1.md`, "Probe
files"), and the hazards it owns. The task that fills a crate keeps its section current. Until
then the crate holds only its name, its probe file is a stub (HEAD, PRUN, END), and every hazard
of its code is **open**.

### orca-eigen

Eigen 5.0.1's operations, as MSVC 14.42 compiles them with the engine's flags (`/O2`,
`/fp:precise`, no `/arch`, so SSE2 packets): the fixed-size vectors and matrices of `Point.hpp`,
`Transform3d` and `Transform3f`, `AngleAxis`, `Quaternion`, `eulerAngles` and the 3x3
`JacobiSVD` that `computeRotationScaling` uses. Probe file `geom.sop1`: EVEC, EMAT, ETRF, EROT and
ESVD (`parity/SOP1.md`), which `crates/orca-eigen/tests/probes.rs` replays; every comparison of
both sets matched on 2026-11-07, debug and release. Filled by o2-geom on 2026-11-07. The hazards
it owns:

| What | Where (Eigen 5.0.1, `Eigen/src/`) | What the translation does |
| --- | --- | --- |
| Reduction order: a tree of SSE2 packets, the lanes folded `(a0 + a2) + (a1 + a3)`, the tail a tree joined last; without packet access one tree | `Core/Redux.h`, `Core/arch/SSE/Reductions.h` | probed: `redux.rs` `redux`, with each type's packet size (2 `double`s, 4 `float`s, 2 `int64_t`s, 4 `int32_t`s) |
| Dot products: up to four packet accumulators, then the rest one by one | `Core/InnerProduct.h` | probed: `redux.rs` `dot` |
| Small matrix products: the rows that fill whole packets left to right, the rest a tree (3-row `double` results differ in row 2, 3-row `float` results in every row) | `Core/ProductEvaluators.h` (`LazyCoeffBasedProductMode`), `Core/AssignEvaluator.h` | probed: `redux.rs` `product_coeff` and `packet_rows`; `Transform * point` is the 4x4 matrix times `(p, 1)` (`Geometry/Transform.h:1347`) |
| `minCoeff(&i)` and `maxCoeff(&i)` search by packets: where a NaN shares a packet with a better coefficient, the index stays on the packet's first coefficient (the full set's `Vec4d` records found it) | `Core/FindCoeff.h` | probed: `redux.rs` `find_coeff` |
| SSE2 has no 64-bit integer compare, and without SSE4.2 `pcmp_lt<Packet2l>` compares the 32-bit halves signed, the low halves too: where the high halves are equal and one low half has its top bit set, the order is wrong (`0x1_8000_0000 < 0x1_0000_0000`). `pmin` and `pmax` select by it, so `cwiseMin` and `cwiseMax` of a `Vec2crd` (both lanes) or of a `Vec3crd` (the first two), the packet steps of an `int64_t` `minCoeff` and `maxCoeff` and the packet compares of their index forms can pick the wrong coefficient for values past 2^31; values in [-2^31, 2^31) compare right | `Core/arch/SSE/PacketMath.h` (`pcmp_lt`, `pmin`, `pmax` of `Packet2l`) | probed through orca-geom's `BoundingBox` (GPLY, coordinates up to 1e10, where 980 comparisons differed with a true compare): `scalar.rs` `pcmp_lt_i64` and `Scalar::packet_lt`, `packet_min`, `packet_max`, used by `Vector::cwise_min`, `cwise_max`, `redux_packets` and `find_coeff` |
| Quaternion products by Eigen's SSE2 kernels, in an order of their own for each type | `Geometry/arch/Geometry_SIMD.h` | probed: `rotation.rs` `QuatProduct` |
| `sin`, `cos`, `atan2` and `pow` in `AngleAxis`, `Quaternion` and `eulerAngles` | `Geometry/AngleAxis.h`, `Geometry/Quaternion.h`, `Geometry/EulerAngles.h` | probed: the static UCRT's, through `orca-crt` (its FMA3 choice is the process's, as in the engine) |
| `JacobiSVD` of a matrix holding a NaN or an infinity leaves U, V and the singular values uninitialised | `SVD/JacobiSVD.h` | open: undefined in C++, so `JacobiSvd3::compute` and `compute_rotation_scaling` return `None` and the caller decides |
| NaN payloads when both operands are NaN: SSE2 returns the first operand's, and MSVC may swap the operands of a commutative operation | every floating-point operation | open: the probes use only the default quiet NaN (`0xFFF8000000000000`); a payload shows only in a printed NaN's sign |

### orca-geom

Clipper 6.4.2, `ClipperUtils` and libslic3r's geometry (points, lines, polygons, polylines,
ExPolygons, bounding boxes, the `Geometry` predicates, angles and transform builders, convex
hulls). Probe file `geom.sop1`: CCLP, COFF, CARE, CCLN, CSIM, CMNK, CINT, GPNT, GPLY, GCLU and
GXFM (`parity/SOP1.md`), which `crates/orca-geom/tests/probes.rs` replays; every comparison of
both sets matched on 2026-11-07, debug and release. Filled by o2-geom on 2026-11-07.

Not translated yet, for the task that first needs them: `Geometry::Transformation` and
`TransformationSVD`, `Geometry::arrange`, `rotation_from_two_vectors`, `transform3d_from_string`,
`rotation_xyz_diff`, `rotation_diff_z`, `is_rotation_ninety_degrees`,
`mat_around_a_point_rotate`, `generate_transform` and `is_point_inside_polygon_corner`;
`Polygon::densify`, `overlaps(Polygons)`, `convex_points`, `concave_points` and
`parameter_by_length`; `Polyline`'s arc fitting (`fitting_result`, empty in all geometry code,
so `Polyline` has none), `Polyline3` and `ThickPolyline`; ClipperUtils' `Surface` overloads,
`union_pt_chained_outside_in`, `order_nodes` and `traverse_pt` (they need ShortestPath), the
`ZPoints` and `Polyline3` overloads and `to_linesf`; `polygons_match` and `expolygons_match`.

The hazards it owns:

| What | Where | What the translation does |
| --- | --- | --- |
| `Int128`'s filtered predicates: the estimate `(a + (1 << 31)) >> 32`, where `1 << 31` is the `int` -2^31, so it subtracts half a word; MSVC's `_mul128` branch for the exact products | `src/libslic3r/Int128.hpp:189-300`, `deps_src/clipper/clipper.cpp:319` | probed (CINT; `SlopesEqual` in every CCLP): `int128.rs`, wrapping `i64` and `i128` |
| `RangeTest`'s `-Pt.x()` on `INT64_MIN`: MSVC wraps it rather than folding the test to `x < -range`, so the point passes, and in the 32-bit range | `clipper.cpp` `RangeTest` | probed (a CCLP case): `wrapping_neg` |
| The local minima sort (`std::sort`, `lm1.y < lm2.y`, ties in introsort's order), the intersections sort (`node2.pt.y < node1.pt.y`) and the maxima sort | `clipper.cpp:975`, `:2517`, `ProcessEdgesAtTopOfScanbeam` | probed: `orca_stl::sort_by` and `orca_stl::sort` |
| The scanbeam: `std::priority_queue<cInt>` | `clipper.cpp:1062` | probed: `orca_stl::PriorityQueue` |
| `ClipperOffset`'s `acos`, `sin`, `cos` and `atan2` | `clipper.cpp:3552-3760` | probed (COFF): the static UCRT's, through `orca-crt` |
| `Round<cInt>`: `FRound` (0.49999999999999994 rounds to 0), then MSVC's truncating cast; `DoRound`'s `std::max<int>(Round(...), 1)` truncates the `int64` to `int`, and a NaN step count (a delta below 0.125 with ArcTolerance 0) gives `INT64_MIN`, so 0, so one step | `clipper.cpp:88-110`, `DoRound`, `DoOffset` | probed (COFF): `fround`, `orca_crt::f64_to_i64`, `as i32` |
| Exceptions: `RangeTest`, an open clip path and `Execute(Paths&)` after an open path throw to the caller; `UpdateEdgeIntoAEL` and `DoMaxima` throw inside `ExecuteInternal`, which catches them and returns false | `clipper.cpp` | probed (CCLP and COFF cases): a panic whose payload is a `ClipperException` (`catch_unwind` and `downcast` catch it, as `catch (clipperException&)` does); the inner two a `Result` |
| Undefined behaviour: `AddPaths` throwing midway leaves the earlier paths' local minima pointing into freed edges; `ProcessHorizontal` reads a maxima iterator uninitialised if the direction flips along a chain of horizontals; `Minkowski` of an empty open path divides by zero | `clipper.cpp` | open: no probe can record it; the translation does what each site's comment says (the last panics), and the engine's callers never get there |
| `BoundingBox`'s `construct` and `merge` take `cwiseMin` and `cwiseMax` of `Point`s, which are Eigen's SSE2 `Packet2l` ones (orca-eigen above), so a box of points past 2^31 can miss a point, and `rotated`, `center`, `size` and `polygon` follow it | `src/libslic3r/BoundingBox.hpp:96-110`, `BoundingBox.cpp:73-100` | probed (GPLY, coordinates up to 1e10): `Point::cwise_min` and `cwise_max`, through `orca_eigen::Scalar::packet_min` and `packet_max` |
| `convex_polygons_intersect`: `rotcalip`'s `dot` and `magnsq` are `int64_t` (the template default) before `cmp_angles` widens them, so for coordinates past about 1e9 (edges past about 2e9) they wrap, the angle comparisons go wrong and the caliper walk can loop forever (the probe found a case with coordinates near 1e10 that never ends) | `src/libslic3r/Geometry/ConvexHull.cpp:136-245` | open: the translation wraps the same way, so it can loop too; `cmp_angles` compares `magnsq * abs(dot) * dot` exactly (a sign and a 192-bit magnitude, as `int256_t` does); GXFM keeps its point sets within 1e8 |
| `orient` takes 64-bit products when every coordinate is at most `0xffffffff`, with no lower bound, so large negative coordinates wrap; otherwise doubles | `src/libslic3r/Geometry.hpp` (`orient`) | probed (GPNT, coordinates up to 1e10): `wrapping_mul`, `as f64` |
| `to_range_pi_pi` subtracts `int(round(a / 2π)) * 2 * π`, the `* 2` in `int` | `Geometry.hpp` | probed (GPNT): `f64_to_i32`, `wrapping_mul` |
| `Point * double`: the member operator, which a non-const point picks, rounds (`Point(x * r, y * r)`); the free one, which a `const Point` picks, truncates. `BoundingBox::offset` subtracts and adds `Point(delta, delta)`, rounded | `Point.hpp`, `BoundingBox.cpp` | probed (GPLY `scale`, `offset`, `rotate`): `Point::mul_round`, `mul_trunc`, `from_f64` |
| ClipperUtils and ExPolygon quirks kept as written: `diff_ex(ExPolygon, ExPolygon, do_safety_offset)` ignores the flag; `union_ex(ExPolygons, Polygons)` adds the polygons as clip paths; `offset_ex(ExPolygon)` unites with EvenOdd; `ExPolygon::contains(Polyline)` drops the `inflated(1)` box it computes; `remove_sticks(ExPolygon)` stops at the first contour that changes (`\|\|`) | `ClipperUtils.cpp`, `ExPolygon.cpp` | translated as written; GCLU probes the polygon and ExPolygons overloads |
| Undefined behaviour: `to_lines(Polylines)` and `to_lines(ExPolygon)` read before the start of an empty path; `keep_largest_contour_only` with no contour of positive area dereferences null; Visvalingam's heap orders `vis_node*` by address and writes past `areas` (nothing calls it) | `Polyline.hpp`, `ExPolygon.hpp`, `ExPolygon.cpp`, `MultiPoint.cpp:254` | open: `to_lines(Polylines)` gives an empty polyline no lines, `to_lines(ExPolygon)` and `keep_largest_contour_only` panic, Visvalingam is left out |

### orca-exact

Boost 1.84.0's `boost::multiprecision::int128_t` (signed magnitude, unchecked, in 32-bit limbs, as
MSVC has no `__int128`) and `boost::rational`. Probe file `arrange.sop1`: XI28 and XRAT
(`parity/SOP1.md`), which `crates/orca-exact/tests/probes.rs` replays; every record of both sets
matched on 2026-11-07 (241 committed, 6561 in the full set). Filled by o2-arrange on 2026-11-07.
The hazards it owns:

| What | Where (Boost 1.84.0, `boost/`) | What the translation does |
| --- | --- | --- |
| `int128_t % 0`: `eval_modulus` by a single limb divides by it unchecked, a hardware integer divide by zero that ends the process (`/` by zero throws `std::overflow_error`) | `multiprecision/cpp_int/divide.hpp:549-570` | `Int128::rem` panics; XI28 records no remainder by zero. `boost::rational` never takes one: its gcd stops at a zero |
| `gcd` of two values of more than one limb whose bit lengths differ by 16 or less takes Lehmer's steps, which multiply U and V by the cosequences in 128 bits: the products wrap, U or V comes out negative or zero, and `eval_lsb` or `eval_msb` throws `std::domain_error` ("Testing individual bits in negative values is not supported - results are undefined.", "No bits were set in the operand."), or the result is not the gcd | `multiprecision/cpp_int/misc.hpp:670-858`, `:1116-1218`, `:326-359` | translated step for step: `Int128::gcd` gives `ExactError::Domain` with the same message, or the same wrong value; probed (XI28's operands near 2^127) |
| `boost::rational` calls that gcd in `normalize()` and in its operators, so its exceptions leave rational arithmetic too | `rational.hpp` | every `Rational` operation passes them on; probed (XRAT) |

### orca-qhull

Qhull 2015.2 (`libqhull_r` 7.2.0 and `libqhullcpp`, built as `qhullstatic` from
`deps_src/qhull`), the one run the engine makes: `its_convex_hull`'s `runQhull("", 3, n, points,
"Qt")` (`TriangleMesh.cpp:1363`), with Qhull's default pre-merge (`C-0`) and the triangulation of
`Qt` (`crates/orca-qhull/README.md`). Its `qsort` calls go through `orca-crt`'s `qsort`, the static
UCRT's own, and its `sqrt`s through `orca-crt` ("O2 sources" above). Probe file `qhull.sop1`: QHUL
(`parity/SOP1.md`), which `crates/orca-qhull/tests/probes.rs` replays through `convex_hull` and the
walk of `its_convex_hull`, and `crates/orca-mesh/tests/probes.rs` through orca-mesh's
`its_convex_hull` and `TriangleMesh::convex_hull_3d`; every record of both sets matched on
2026-11-07 (199 committed, 3619 in the full set), debug and release. Filled by o2-mesh on
2026-11-07. The hazards it owns:

| What | Where (`deps_src/qhull/src/`, `src/libslic3r/`) | What the translation does |
| --- | --- | --- |
| `qh_gethash` adds the addresses of a ridge's vertices, which ASLR moves from run to run, so where a new facet sits in the hash table varies. A ridge of two new facets is matched by its vertices wherever they sit; a ridge that more than two new facets share (a duplicate ridge) is resolved in table order: `qh_matchneighbor` takes the first candidate on the chain, and `qh_matchduplicates` pairs the facets as the chain holds them, so the hull can differ between two runs of the engine | `libqhull_r/poly_r.c:405-463` (`qh_gethash`), `:724-807` (`qh_matchneighbor`), `:891-901`, `poly2_r.c:2088-2173` (`qh_matchduplicates`); `merge_r.c:1275-1340` (`qh_hashridge`, `qh_hashridge_find`), where `qh_find_newvertex` (`:818-831`) asks only whether a match exists, so the order does not count | open: the vertex ids stand in for the addresses (`poly.rs`); the probe's sets never reach a duplicate ridge (`qh_matchduplicates` is never called on the full set) |
| `qh_triangulate` deletes a non-simplicial facet none of whose triangles takes its hyperplane over (all of them degenerate) with its normal, which the triangles still point to. `its_convex_hull` reads that freed normal (`getBaseT()->normal`), whose first coordinate Qhull's memory pool has overwritten with the next free block's address (a positive subnormal number, or 0); nothing in the run allocates after it | `libqhull_r/poly2_r.c:2871-2876`, `:2905-2909`, `poly_r.c:257` (`qh_delfacet`); `TriangleMesh.cpp:1407` | open: the translation writes +0.0 in that coordinate and keeps the other two, so the sign of the dot product differs only where the other two terms sum to exactly 0 and the free list was not empty; not reached by the probe's sets |
| `qh_normalize2`, for a normal it cannot divide (`qh_divzero`), takes the largest coordinate from `qh_maxabsval`, which returns NULL when all three are NaN; the engine dereferences it and dies | `libqhull_r/geom_r.c:864-865`, `geom2_r.c:938-951` | open: `Error::Unported`, which `its_convex_hull` takes as a failed run (an empty mesh and the error logged); never reached (`qh_maxabsval` is never called on the full set) |
| Messages: libqhullcpp's `qh_fprintf` appends each message to the run's text (`vsnprintf` into 3000 bytes), a thrown `QhullError` takes the text away, and `~Qhull` writes what is left to `std::cerr` after `"\nQhull output at end\n"`: after a hull that succeeded, only the narrow-hull warning (9375, its cosine `%.16f`) | `libqhullcpp/QhullQh.cpp:201-237`, `Qhull.cpp:95-105`; `libqhull_r/user_r.c:415`, `poly2_r.c:1859` | probed (QHUL `cerr`, 11 records of the full set): `orca-fmt`'s `printf`; orca-mesh writes it with `log::write_stderr`, whose stream turns `\n` into CRLF as the C runtime's text-mode `stderr` does |
| `its_convex_hull` writes `indices[cnt]` before it tests `cnt ++ == 3`, so a facet of four vertices or more writes past its `Vec3i32` | `TriangleMesh.cpp:1394`, `:1397`, `:1399` | never reached: after `Qt` every facet has three vertices; the translation stores only the first three |
| Qhull's memory pool (`qh_memalloc`, `qh_memfree`, the `qsort` of its sizes) decides only what a freed block holds (the second row); `run_id`, drawn from a generator seeded with `time()`, only `FO` prints | `libqhull_r/mem_r.c` (`:362`), `global_r.c:1864-1866` | left out, with the statistics and the trace output: none of them changes a result. So the `mem_r.c:362` and `global_r.c:1864-1866` rows of "O2 sources" need nothing |

### orca-mesh

admesh (`deps_src/admesh`) and libslic3r's `TriangleMesh` (`TriangleMesh.cpp`,
`MeshSplitImpl.hpp`): the STL reader, the repair on import, the shared vertices, the statistics,
the transforms, the bounding boxes, the split and the 2D hull. Probe files `mesh.sop1`: MCAS and
MSTD (`parity/SOP1.md`), which `crates/orca-mesh/tests/probes.rs` replays; and, for the C
runtime's stdio that admesh reads with (`orca-fmt`'s `stdio` and `scanf`), `cstdio.sop1`: CSCN
and CFIL, which `crates/orca-fmt/tests/probes.rs` replays. Every record of both sets of both files
matched on 2026-11-07. Filled by o2-mesh on 2026-11-07. `its_convex_hull` and
`TriangleMesh::convex_hull_3d` run orca-qhull: the probe test replays `qhull.sop1`'s QHUL records
through them too, and orca-qhull's section above has their hazards. The hazards it owns:

| What | Where (`deps_src/admesh/`, `src/libslic3r/`) | What the translation does |
| --- | --- | --- |
| The CR box: `fscanf(" solid %[^\n]")` into `char solid_name[256]` overruns the stack for a `solid` line of 256 bytes or more (any file with CR line ends), and the engine dies with 0xC0000409 | `stlinit.cpp:164-165` | a defined answer (`parity/CORPUS.md`, "An engine fault: 20mmbox-CR.stl"): `stl_open` refuses the file with `StlError::OverlongSolidLine`, as `orca-fmt`'s `fscanf` stops with `Overflow` there (`tests/stl_open.rs`); the probes make no such call (CFIL call 8) |
| The `MW` name: `strstr(solid_name, "MW")`, then `sscanf(mw_position + 3, "%s %s %s")` into `char[16]`, `char[128]` and `char[16]` with no widths, so a longer word overruns the stack (a name such as `MW_cover_plate_with_holes`), and `mw_position + 3` is past the string when `MW` ends it | `stlinit.cpp:167-187` | open: undefined in the C++, not probed. What it reads reaches only the progress callback, which the engine does not pass, so the Rust leaves it out and reads on, where the C++ may die as on the CR box |
| A failed `fgets` (at the end of the file) leaves `buf` as it was: the facet's line read before, or an uninitialised stack array before the first | `stlinit.cpp:245-252` | a failed read matches neither `endloop` nor `endfacet`, which is the C++'s answer wherever `buf` holds a line read before (an `endfacet` line cannot pass for `endloop`, nor the reverse); probed (MCAS files cut short, CFIL) |
| An ASCII facet's two extra bytes are never set (`stl_facet facet;` uninitialised) | `stlinit.cpp:212` | zeros; nothing reads them for an ASCII mesh, and MSTD leaves them out |
| `ftell` returns a `long`, which a file of 2 GiB or more passes | `stlinit.cpp:60` | `orca-fmt`'s `ftell` gives -1, C's failure value, so the file is refused for its size; not probed (no such file) |
| A text stream's `fread` of 512 bytes or more at the stream's start leaves raw file bytes after what it returns (the OBJ reader reads with `fread`) | the static UCRT | `orca-fmt` writes only the bytes read: C leaves the rest unspecified, and no reader looks at it (CFIL records only the items read) |
| `stl_check_facets_exact` on a mesh left with no facets (every one degenerate) sizes its hash table from `0 * 3 * 2 - 1` in `size_t`, so the largest prime: 1610612741 chain heads, 12.9 GB allocated and filled | `connect.cpp:125`, `:164-171` | the heads are allocated at the first insertion, so never there; the result is the C++'s whenever its allocation succeeds |
| `remove_facet` stops early on broken connectivity (it logs "this is wrong"), leaving `facet_start` and `neighbors_start` longer than `number_of_facets` and neighbour indices past it | `connect.cpp:513-546` | kept: the arrays stay that long, every loop runs to `number_of_facets`, MSTD writes the arrays whole |
| `stl_fix_normal_directions` marks facets in `norm_sw`, `number_of_facets` bytes long, and reads and writes it at those neighbour indices past its end | `normals.cpp:134-213` | open: a heap overrun, undefined; the Rust array covers the facet arrays' whole length, and its entries past `number_of_facets` start unfixed |
| `get_volume` reads `facet_start[0]` when every facet was removed | `util.cpp:276-279` | 0, as the loop does not run (the vector keeps its buffer after `pop_back`, so the C++ reads stale memory and does not fault) |
| `create_face_neighbors_index(ex_tbb, ...)`, which `its_split`, `its_is_splittable` and `its_number_of_patches` use without an index: tasks claim each other's edges with no synchronisation, so with more than one thread the index depends on timing | `MeshSplitImpl.hpp:224-270`, `TriangleMesh.cpp:1572` | probed under the oracle's threading (one thread in an arena of 8; MCAS is replayed in `orca_tbb::Arena::serial(8)`); in a parallel arena the Rust races as the C++ does, through atomics |
| `x *= -1.f` and `x *= -1.0` on a `float` (`mulss`, `mulsd`) keep a NaN's sign and quiet it, where `-x` flips the sign | `TriangleMesh.cpp:316-334`, `util.cpp:210-242` | `admesh::times_minus_one` |
| `m.block(0, 0, 3, 3).determinant()` is a block of runtime size, so Eigen takes it by partial-pivoting LU, not by the 3x3 cofactor formula | `TriangleMesh.cpp:340`, `:352` | probed (MCAS `transform` calls): `eigen_lu.rs` |

### orca-zip

miniz 2.1.0's read path, as the 3MF reader uses it through `miniz_extension.cpp`. Probe file
`zip.sop1`: ZCRC, ZINF, ZARC and ZERR (`parity/oracle/probe/zip.cpp`), which
`crates/orca-zip/tests/probes.rs` replays; every record of both sets matched on 2026-11-08. Filled
by o2-model. The hazards it owns (lines of `deps_src/miniz/miniz.c`):

| What | Where | What the translation does |
| --- | --- | --- |
| `mz_zip_archive_file_stat::m_time` comes from `mktime` of the DOS date and time, in the local time zone and its daylight-saving rules (class P) | `mz_zip_dos_to_time_t`, `:3318-3329`, `:4145` | left out: the 3MF reader never reads it, and `mz_zip_reader_extract_to_file`'s `mz_zip_set_file_times` (which the command line never calls) is not translated |
| The central directory's "stored" check reads the method and the file time as one 32-bit word (`MZ_READ_LE32(p + MZ_ZIP_CDH_METHOD_OFS)`), so a stored entry whose time is not zero passes with sizes that differ | `:3771` | kept (ZARC has such archives) |
| The end-of-directory search steps back by 4093 bytes from `archive_size - 4096`; between 4097 and 8188 bytes it reaches a negative offset, which `MZ_MAX(..., 0)` of an `mz_int64` keeps negative, and the read there fails | `mz_zip_reader_locate_header_sig`, `:3505-3550` | kept: the read at a negative offset gives no bytes and the search fails with `MZ_ZIP_FAILED_FINDING_CENTRAL_DIR` (ZARC) |
| `memcpy` of no bytes from an offset past the end of the archive's memory (`mz_zip_mem_read_func`) | `:3880-3886` | no read (a slice past the end would panic in Rust); ZARC's full set reaches it |
| A callback extraction's end reports a CRC mismatch as `MZ_ZIP_DECOMPRESSION_FAILED`, where the extraction to memory says `MZ_ZIP_CRC_CHECK_FAILED` | `mz_zip_reader_extract_to_callback`, `:4768-4788` | kept (ZARC) |
| `tinfl`'s match copy of 9 or more bytes no longer than the distance is `memcpy` of 8 bytes at a time (`MINIZ_UNALIGNED_USE_MEMCPY`), which MSVC makes one load and one store: in a ring smaller than the distance the source can overlap the output from behind, and the copy differs from a byte copy | `tinfl_decompress`, `:2727-2750` | the same load and store of 8 bytes (ZINF's ring of 1024 bytes in the full set); the zip reader's ring is the whole window, where it never overlaps |
| The names are sorted with miniz's own heap sort (not `qsort`), case folded with `MZ_TOLOWER` of ASCII | `mz_zip_reader_sort_central_dir_offsets_by_filename`, `:3453-3503` | the same heap sort, step for step; the order of equal names is the C's (ZARC's queries) |

### orca-xml

expat 2.5.0, the dependency build's (`src/deps/EXPAT/expat`, without `XML_DTD`, `XML_NS`,
`XML_CONTEXT_BYTES` and `XML_ATTR_INFO`), as `bbs_3mf.cpp` drives it. Probe file `xml.sop1`: XPRS
and XERR (`parity/oracle/probe/xml.cpp`), which `crates/orca-xml/tests/probes.rs` replays; every
record of both sets matched on 2026-11-08 (1196 committed, 7755 in the full set). Filled by
o2-model. Boost 1.84's `property_tree` on rapidxml, as the 3MF reader reads its cut information
and height ranges with it, is in `orca-xml` too: probe file `ptree.sop1`, PTXR and PTGT
(`parity/oracle/probe/ptree.cpp`, orca-ptree-probe, 2026-11-10), which
`crates/orca-xml/tests/ptree.rs` replays; every record of both sets matched on 2026-11-11 (139 PTXR
and 60 PTGT committed, 4249 and 82 in the full set; orca-ptree-port). Its hazards are the
`read_xml`, rapidxml and `get<T>` rows of the 3MF table under orca-model. The hazards expat owns
(lines of `xmlparse.c`):

| What | Where | What the translation does |
| --- | --- | --- |
| After an error, or after a final chunk that parsed, `XML_Parse` returns with its position and event pointers still in the caller's chunk; the reader's `XML_GetCurrentLineNumber` then counts the lines between them, in miniz's buffer, which still lives | `XML_Parse`, `:1896-1925`; `XML_GetCurrentLineNumber`, `:2290-2300` | the parser keeps a copy of the bytes from the one pointer to the other (`Buf::Saved` in `parse.rs`); XPRS's errors fed in chunks prove the lines and columns |
| Without `XML_DTD` there is no amplification accounting: internal entities that refer to each other many times expand without limit, in time and memory | `accountingDiffTolerated`, `:7728` (compiled out) | kept: no limit, as in the C++ build. The probe keeps its expansions small |
| The lookup tables (entities, element types, attribute names) are hash tables with a salt from `rand_s` (its row under "Random engines, thread-local generators, the time and the user") | `:832-847`, `:873` | ordered maps (`BTreeMap`): nothing the reader sees depends on a table's order, so no salt is needed |
| A failed allocation gives `XML_ERROR_NO_MEMORY` at about 26 sites | `MALLOC` and `REALLOC` | not modelled: Rust stops on a failed allocation. The size checks that overflow an `int` or `XML_Size` do give `XML_ERROR_NO_MEMORY`, as the C's do |
| Lengths are `int` and lines and columns `XML_Size`, an `unsigned long` of 32 bits | `XML_Parse`, `XML_GetBuffer`, `XML_GetCurrentLineNumber` | the same widths: a length is cast to `i32` as the reader's `(int)n` is, and a line or column wraps at 2^32 |
| Not translated, as the reader never calls or reaches them: `XML_ResumeParser` and the resumption of a suspended internal entity (`internalEntityProcessor`, `:5775`), external entities (`externalEntityInitProcessor`, `:2628`), the handlers the reader leaves unset and `XML_SetUnknownEncodingHandler`, `XML_GetCurrentByteIndex`, `XML_GetCurrentByteCount`, `XML_GetInputContext`, `XML_GetIdAttributeIndex`, `XML_ParserReset` and `XML_SetEncoding` | | a resumable stop is translated in `Ctl::stop_parser`, and nothing resumes it. The three handlers are always called, where the C tests each for `NULL`: the reader sets all three |

### orca-placeholder

The placeholder parser's boolean expressions (`PlaceholderParser::evaluate_boolean_expression`),
for the presets' compatibility conditions: the grammar with Spirit's semantics, the values and
operators, `boost::regex` 1.84 with MSVC's narrow `w32_regex_traits`, and the error texts with
`process_error_message`. Probe file `placeholder.sop1`: PHCF and PHEV (`parity/SOP1.md`), which
`crates/orca-placeholder/tests/probes.rs` replays; every evaluation of both sets matched on
2026-11-07 (740 committed, 3260 in the full set with every condition of the vendor profiles),
debug and release. Filled by o2-front on 2026-11-07. The hazards it owns:

| What | Where (OrcaSlicer 2.4.2, Boost 1.84.0) | What the translation does |
| --- | --- | --- |
| Boost.Regex's limit on a match's work (`error_complexity`, `std::runtime_error`), which the presets stage would catch and count as compatible | `boost/regex/v5/perl_matcher_common.hpp` (`estimate_max_state_count`) | open: not modelled; the crate matches by simulating the pattern, which never runs out. The profiles' patterns are short and match far below the limit (all 3260 matched) |
| A class (`\w`, `[[:alpha:]]` and the rest) tested against a byte past ASCII: `w32_regex_traits` asks `GetStringTypeExA` with the user's code page | `boost/regex/v5/w32_regex_traits.hpp` | declined (`Unsupported`): the result rests on the machine's locale. A byte past ASCII as a literal, in a range or under `.` is compared as a byte, as Boost compares it |
| The ASCII classes come from `GetStringTypeExA` too: `\s` holds 0x09-0x0D, 0x20 and 0x1C-0x1F (`C1_SPACE`), `\h` the same less the vertical ones, `\v` 0x0A-0x0D | `w32_regex_traits.hpp` (`isctype`, the `\h` and `\v` masks) | probed (`"\x1c" =~ /\s/` and the rest of the hand-written set) on this machine's code page; ASCII is the same in every Windows code page |
| An error message's line goes through `boost::nowide::narrow(widen(line))`: bytes that are not UTF-8 become U+FFFD, each as `utf_traits<char>::decode` delimits it | `PlaceholderParser.cpp` (`process_error_message`), `boost/nowide/utf/utf.hpp` | probed: `parser.rs` `nowide_round_trip`, with the invalid sequences of the hand-written set (a lone continuation, an overlong form, a truncated sequence) |
| A percentage over a line width (`ratio_over` ending in `line_width`) or a line width itself: `Flow::extrusion_width` | `PlaceholderParser.cpp` (`scalar_variable_to_expr`, `vector_element_to_expr`) | declined: `Flow` is not ported |
| An error at a default-constructed iterator: `process_error_message` throws `std::length_error`, which the presets stage does not catch | `PlaceholderParser.cpp` (`process_error_message`) | declined, as the probe's outcome 2 says the C++ code throws |
| `INT_MIN / -1` and `INT_MIN % -1`: undefined, a hardware exception on x64 | `PlaceholderParser.cpp` (`expr::operator/`, `operator%`) | declined |
| An element of a points vector (`to_string(Vec2d)`), `digits()` past its 256-byte buffer, a string literal with a lead byte from 0xF8 (where `get_utf8_sequence_length` and the grammar count differently), `interpolate_table` of an empty table | `PlaceholderParser.cpp` | declined |
| `random(a, b)` in a condition: `evaluate_boolean_expression` gives no context data, so it fails with "Random number generator not available in this context." and no generator is drawn from (the generator of "Clocks and generators" above is the custom G-code's, phase O4) | `PlaceholderParser.cpp` (`random()`) | translated: the same error |
| A `double` printed into a string (`"v" + 1/3.0`): `std::ostringstream`'s `%g` with 6 digits | `PlaceholderParser.cpp` (`expr::to_string`) | probed: through `orca-fmt` |

### The presets stage's flushing volumes and conditions (orca-cli, o2-front)

`crates/orca-cli/src/presets.rs` completes the merged configuration as `cli/presets.cpp` does
(2026-11-07): the compatibility conditions (through orca-placeholder), `printer_model_id`,
`is_bbl_printer`, `curr_bed_type` and the flushing volumes. The hazards it owns:

| What | Where (OrcaSlicer 2.4.2) | What the translation does |
| --- | --- | --- |
| `GenericFlushPredictor` keeps each dataset's predictor in a static map for the process's life; the translation keeps them for one computation of the volumes | `src/libslic3r/FlushVolPredictor.cpp:310-327` | the same results: a run computes the volumes once, and each data file is read once either way |
| The predictors' data files are read with `operator>>` into `float` (`num_get`) | `FlushVolPredictor.cpp:239-253` | the shipped `resources/flush` files (decimal numbers) are read as the C++ reads them; a number past `float`'s range or in a form `num_get` collects differently (hexadecimal, `inf`) is open |
| A condition that does not parse: the printer's check logs it (Boost.Log, `warning`), the process's prints it with `printf` | `src/libslic3r/Preset.cpp:771-845` | translated: the printer's to stderr at `SKETCH_ORCA_LOG_LEVEL` 2 or more, the process's to stdout |
| `DynamicPrintConfig::get_index_for_extruder` is private in `crates/orca-cli/src/config.rs`, so `presets.rs` has its own copy (`printer_variant_index`, with `split_compress` and the variant string) | `src/libslic3r/PrintConfig.cpp` | one translation in two places, which must change together |

### The model stage and the info answer (orca-cli, orca-info-load)

`crates/orca-cli/src/run.rs`, `model.rs` and `info.rs` translate `cli/run.cpp`'s model stage
and info answer (2026-11-11): `load_model`, the check of the objects the job lists, the orient
decline, `check_solid_objects` and `info_json`. `log.rs` prints Orca's log on stderr as
`cli/main.cpp` sets it up. A layers or slice job goes on to the placement (the next section) and
is declined once its objects are placed, so what the model's reading logs reaches stderr before
its decline, as it does in the C++ run. The hazards they own:

| What | Where | What the translation does |
| --- | --- | --- |
| The OBJ reader logs at error what it refuses and a material library it cannot find: `load_obj: failed to parse <path>` (with the reason for a face of more than four or fewer than three corners, or a corner outside the vertices), `load_obj:load_mtl: failed to parse <library>`, `load_obj: failed to load mtl_path:<library>` (and the load goes on), `load_obj: This OBJ file couldn't be read because it's empty. <path>`, `ObjParser: Unknown command: <the line's first character>` for each line of a command it does not know, and `ObjParser: Excessive line length` or `MtlParser: Excessive line length`. The command line prints them on stderr, where certify compares them | `src/libslic3r/Format/OBJ.cpp:32`, `:59`, `:65`, `:79`, `:84`, `:126`, `:207`; `Format/objparser.cpp:370`, `:596`, `:636` | translated (orca-obj-log, 2026-11-12): orca-model's `obj.rs` logs each record through orca-mesh's `log` in the C++'s words and order, and orca-cli prints them (`log.rs`). `crates/orca-cli/tests/info.rs` holds `model-obj-mtl-missing-info` to one run of the staged C++ program, its stderr line included, and its layers job to the same line before the decline. The unknown command is one byte, which the C++ writes as it is; orca-mesh's `log` takes text, so a byte past 0x7F becomes U+FFFD. The `Out of memory` records stay untranslated, as the next row says |
| A failed allocation while the model is read: `read_from_file` throws `std::bad_alloc`, which `load_model` answers as `model-unreadable` ("cannot read the model <path>: bad allocation"); the OBJ parsers catch it, log `ObjParser: Out of memory` or `MtlParser: Out of memory` and go on with what they read (class P: it hangs on the memory the machine gives) | `cli/run.cpp:265-272`; `Format/objparser.cpp:603-605`, `:642-644` | not translated: Rust ends the process at a failed allocation (its allocation error handler), with no result. No corpus model comes near the memory a run has |
| A model of no object: `cli/run.cpp:278-279` refuses it ("the model <path> has no objects"), but `read_from_file` throws first for every reader ("The supplied file couldn't be read because it's empty") | `cli/run.cpp:278-279`; `src/libslic3r/Model.cpp:346-355` | translated as written (`model.rs`); no file reaches it |
| The 3MF reader's backup folder (orca-model's rows below) stays in the work folder after the run, as `~Model`'s removal is not translated; the C++ removes it on its backup thread, if that thread runs before the process ends (class P) | `Model.cpp:176-183` | the folder is made under the job's work folder (`run.rs`, `set_up_folders`, which sets the temporary folder as `cli/run.cpp:240` does) and left there. The parity runs leave the `work` folders out of what they compare (`scripts/lib/parity/same.ts`) |
| The CR box (orca-mesh's and orca-model's rows): the C++ engine dies inside `load_stl` | admesh `stlinit.cpp:164-165` | `model-unreadable`, "cannot read the model <path>: " and orca-model's `OverlongSolidLine` text, exit -6, nothing on stdout or stderr (`crates/orca-cli/tests/info.rs`); the corpus holds the case as an engine fault (`parity/CORPUS.md`) |

### Placement and the parity knobs (orca-cli, orca-info-place)

`crates/orca-cli/src/place.rs` translates `cli/run.cpp` from the model stage to the front-objects
boundary (2026-11-11): the objects' settings, the custom G-code, the prime tower's size and place,
the excluded areas, the arrange and the build volume check. `oracle.rs` reads the oracle's knobs
(`parity/oracle/README.md`) and writes the front-objects dump, for parity work only.
`crates/orca-cli/tests/place.rs` holds twelve placements to the oracle's dumps byte for byte (the
corpus's painted Bambu project among them), the corpus's placement refusals to their recorded
results, and the knobs' refusals to the oracle's.
The hazards they own:

| What | Where | What the translation does |
| --- | --- | --- |
| The knobs' texts: the oracle reads them with `getenv`, in the process's ANSI code page (class R) | oracle patches 0001 to 0005 | read as Rust reads the environment, the UTF-16 made UTF-8 (`oracle.rs`, `knob`). Numbers, `0` and `1` read the same either way; a dump folder named with characters outside the code page is another folder, or none, in the oracle |
| The RUN record's machine fields: the arena's concurrency, the thread limit (the default concurrency without the thread knob) and the CRT's FMA3 state, the CPU's choice without `FMA3=0` (class P: the machine) | `OracleDump.cpp` (RUN, `parity/SOP1.md`); oracle patches 0003 and 0004 | reported as this machine and build have them: orca-tbb's default concurrency, and `orca_crt`'s FMA3 state on Windows x64, -1 elsewhere. A dump taken without those knobs differs between machines; `tests/place.rs` writes the machine's values into the recorded RUN before it compares |
| `SKETCH_ORCA_ORACLE_TIME` and `SKETCH_ORCA_ORACLE_SEED` pin the oracle's clock and generators (class P) | oracle patches 0001 and 0002 | checked, and reported in RUN, only: nothing up to the front-objects boundary reads the clock or those generators into a result or a dump. The 3MF reader's backup folder (orca-model's `Model::get_backup_path`) is named by the real local time, where the oracle's has the pinned time; it is in no result or dump, and the parity runs leave the work folder out. The steps past the boundary that read them (the G-code header, the placeholder parser's time and `random()`, fuzzy skin, Lightning's `srand`) must take the knobs when they are ported |
| An arrange error that is no C++ exception (class R): `ArrangeError::Undefined`, where the C++ reads past a vector (orca-arrange's rows below), and `ArrangeError::RotationsNotTranslated`, a path left untranslated that the command line never takes (`arrange_objects` sets `allow_rotations = false`, as `cli/run.cpp` does) | `cli/run.cpp:967-1036` | `internal-error` with the error's text (`place.rs`, `arrange_failure`); at `Undefined` the C++ has undefined behaviour. Arrange's exceptions give what the C++'s catch gives: `std::bad_alloc` `out-of-memory` ("out of memory"), any other `internal-error` with its `what()` |
| A setting read through a pointer the C++ does not check: `opt_float`, `opt_int` and `opt_string` of a missing key or another type, `option<ConfigOptionFloats>(key, true)` of the tower's place in another type, and the `dynamic_cast`s of `check_build_volume` (class R; undefined behaviour in the C++) | `cli/run.cpp:680-913`, `:1040-1068` | a panic naming the setting, so the crash result says `panic at place.rs:<line>` where the C++'s says `exception 0xC0000005 at <module>+<offset>`. No job is known to reach them: the presets stage gives the configuration every key they read, each with its definition's type |
| The BRIM and PNTV records with content: brim ears and painted facets, which only a 3MF model carries (class R) | `OracleDump.cpp` (patch 0006), `parity/SOP1.md` | written as `OracleDump.cpp` writes them. Painting with content is held to the oracle by `parity certify` (model-3mf-bambu-layers' front-objects dump, whose PNTV records carry its support, seam, colour and fuzzy-skin painting, is among the 148 dumps certify compares) and, from 2026-11-12 (orca-o2-review-tests), by `tests/place.rs`; only the BRIM record with content is open, as no corpus 3MF carries brim ears |

### orca-arrange

libnest2d, `Arrange.cpp`, NLopt 2.5.0's subplex and Nelder-Mead, and Boost.Geometry's R*-tree.
Probe file `arrange.sop1`: NLOP, NOPT, RTRE, NFPC, NGEO and ARNG (`parity/SOP1.md`), which
`crates/orca-arrange/tests/probes.rs` replays; every record of both sets matched on 2026-11-07
(279 committed, 7931 in the full set). Filled by o2-arrange on 2026-11-07; `ModelArrange.cpp` (the
inflations, `get_shrink_bedpts` and the `ArrangePolygon` setter) is o2-model's. The hazards it
owns (NLopt's paths are under NLopt src/, libnest2d's under
`deps_src/libnest2d/include/libnest2d/`):

| What | Where | What the translation does |
| --- | --- | --- |
| NLopt's clocks: `nlopt_optimize_` seeds NLopt's generator from `time(NULL)` and the thread id (`nlopt_srand_time_default`) and reads `nlopt_seconds()` into `stop.start` | `api/optimize.c:414`, `:433`, `util/stop.c:90`, `api/general.c:128-136` | class O: neither reaches a result. The Rust reads the clock only for a `maxtime > 0`, which no caller sets (libnest2d sets no time limit), and keeps no generator, as neither subplex nor Nelder-Mead draws from it |
| The qsort path: subplex sorts its subspace indices by `abs(dx)` with `nlopt_qsort_r`, NLopt's own sort (an exchange sort below ten elements, else a quicksort on the median of three), whose ties (0) land where it leaves them; it never calls the CRT's `qsort`, `qsort_s` or `qsort_r` | `algs/neldermead/sbplx.c:119`, `util/qsort_r.c:68-104` | translated as written (`nlopt::qsort_r`), not `orca_crt::qsort`; probed (NLOP, NOPT) |
| Nelder-Mead keeps its simplex in NLopt's red-black tree, ties broken by the key's address (`k1 - k2`) | `algs/neldermead/nldrmd.c:38-42`, `util/redblack.c` | translated node for node, ties by row; probed (NLOP) |
| With dimensions of equal bounds eliminated, the algorithm runs on a copy of the options with a stop flag of its own, so a `force_stop()` on the caller's `nlopt::opt` goes unseen | `api/optimize.c` (`elimdim`) | the same; probed (NLOP's chosen cases) |
| An objective that throws inside `nlopt::opt::optimize`: `myfunc` keeps only the exception's kind and forces a stop, which clears the message, so `mythrow` throws the kind's default ("nlopt invalid argument", "nlopt failure", MSVC's "bad allocation") | `api/nlopt-in.hpp` (`myfunc`, `mythrow`) | the same (`optimizer.rs`); probed (NOPT, each kind) |
| The R*-tree's queries return elements in tree order (the "Orders" table above has the objective function sum over them) | `src/libslic3r/Arrange.cpp:78`, `:530-560` | the tree is not built: `SpatIndex` keeps a list in insertion order with the tree's answers (each query's set, `bounds()`, `size()`). `Arrange.cpp` reads a query's results only through a strict `<` minimum and `empty()`, so their order never reaches a result; probed (RTRE, ARNG) |
| `within(polygon, polygon)`, which Boost computes with its relate machinery, for a bed that is neither a box nor a circle (the bin check, the bottom-left objective, the excludes dropped outside the bed) | `utils/boost_alg.hpp:378-381` | not translated: the exact predicate in 128-bit integers, which agrees with Boost wherever Boost's double side tests are exact; probed (NGEO, ARNG's polygon beds), open past that |
| An `ArrangePolygon` of fewer than three points makes no item, yet the results are written back by position: each later polygon gets the next one's result and the last keeps its own | `Arrange.cpp` (`process_arrangeable`, `arrange`) | kept; probed (ARNG's chosen run with a two-point polygon) |
| A triangle alone on a plate: libnest2d's hull of a path of three points is empty (`edges <= 3`), and the offset by zero every item gets (`inflation(0)` still sets `has_inflation_`) takes a collinear fourth point off. `finalAlign(Box)` gives `nfpInnerRectBed` the empty hull, whose inverted box is past Clipper's range, and Clipper throws inside `~_NofitPolyPlacer`, which is `noexcept`: `std::terminate`. With fixed items on the plate, `correctNfpPosition` first reads the empty item's vertices (`*end()`) | `geometry_traits.hpp:1049-1056`, `nester.hpp:308-312`, `:434-444`, `placers/nfpplacer.hpp:338-373`, `:552-554`, `:1069` | a panic, `std::terminate: <what> left ~_NofitPolyPlacer`: `firstfit.rs`'s `destroy` turns any error or panic of `finalAlign` in a destructor into one (unit test `a_lone_triangle_ends_the_run_in_the_final_alignment`). ARNG inflates every triangle |
| An item that fits an empty plate but scores 1e7 or more (or below 0) on every plate never meets the `MAX_NUM_PLATES` test, so plates are opened one after another, and its second failure on a plate with nothing packed preloads `fixed_bins[placers.size() - 1]` past the end of `fixed_bins`: undefined behaviour | `selections/firstfit.hpp:116-200` (the read at `:179`) | `ArrangeError::Undefined` at that read, so the Rust ends there; ARNG keeps to inputs that end (below) |
| The placers live in a `std::vector` reserved for as many placers as items. `_NofitPolyPlacer` is not nothrow-movable (`Slic3r::Point` declares its copy constructor), so when a run opens more plates than that, MSVC copies the placers and destroys the old ones in order, each destructor's `finalAlign` moving the items its copy shares | `selections/firstfit.hpp:43`, `:192` | kept, with MSVC's growth (half again) |
| `ArrangeParams::progressind` defaults to printing `st=<n>, <name>` to `std::cout` | `src/libslic3r/Arrange.hpp:146-148` | no default: `ArrangeCallbacks` holds none, and `stdout_progressind` prints what the C++ default prints, for a caller that keeps the C++ default (o2-model's `ModelArrange`) |
| `allow_rotations` runs libnest2d's rotating calipers (`fitIntoBoxRotation`, `minAreaBoundingBox` over `boost::rational<int128_t>`) | `Arrange.cpp:945-1005` | not translated: `ArrangeError::RotationsNotTranslated`, before anything changes; the command line sets `allow_rotations = false` (`cli/run.cpp:994`) |

ARNG's generated runs keep to inputs on which the C++ is defined and ends
(`parity/oracle/probe/arrange/runs.cpp`): fixed items and excluded regions on box beds only,
convex polygon beds when printing by object, fixed items on the bed, and every triangle inflated
(an uninflated one by 0.5 mm).

### orca-model

`Model::read_from_file` with the STL reader, `Model` and its objects, parts and instances,
`Geometry::Transformation`, a part's painting (`FacetsAnnotation`), `ModelConfig`, `BuildVolume`
with the circle fits, and what the command line does around arrangement (`ModelArrange.cpp`,
`Arrange.cpp`'s bed and inflation updates). Probe file `model.sop1`: MXFM, MFAC, MCFG, MCIR,
MBVL, MLOD, M3MF, MARP and MTMP (`parity/oracle/probe/model.cpp`), which
`crates/orca-model/tests/probes.rs` replays; every record of both sets matched on 2026-11-07. The
OBJ reader followed on 2026-11-09 (MLOD's OBJ files and fixtures, every record of both sets). The
3MF reader's records (M3MF) came first, on 2026-11-10, one test for each of its port's three pieces;
piece 1 (the container, its relationships and the root model) followed the same day, and every M3MF
record of piece 1 of both sets matched (13 committed, 15 in the full set). Piece 2 (the config
entries) followed on 2026-11-11 (orca-3mf-port-2), and every M3MF record of pieces 1 and 2 of both
sets matched (13 and 13 committed, 17 and 45 in the full set). Piece 3 (the sub-model files and the
assembly) followed the same day (orca-3mf-port-3), and every record of both sets matched (piece 3's
M3MF records: 13 committed, 57 in the full set). orca-3mf-probe-3 added 17 records to the full set
the same day; with `cut_id`'s attributes read right to left (orca-3mf-port-4) every one matched (the
full set's M3MF records: 19, 60 and 57 by piece). Filled by o2-model on 2026-11-07. The hazards it
owns:

| What | Where (`src/libslic3r/`) | What the translation does |
| --- | --- | --- |
| The CR box (orca-mesh's section): an ASCII STL whose `solid` line passes 255 bytes kills the engine inside `load_stl` | `Format/STL.cpp`, admesh `stlinit.cpp:164-165` | `ReadError::OverlongSolidLine`, which the caller answers as a fault; the probe records no such file |
| A part of fewer than two triangles gets no convex hull (`ModelVolume`'s constructor computes it from two up), and the C++ dereferences the null hull wherever it reads it (an instance's hull box, the print volume state, the arrange polygon) | `Model.hpp:1074`, `:1152`, `Model.cpp` | `ModelVolume::get_convex_hull` panics. The command line refuses parts of fewer than four facets or a flat box first (`check_solid_objects`), so it never gets there; MLOD records such a file (a single triangle) only up to the loaded model |
| `TriangleSplittingData::update_used_states` reads the nibble after a `C` (`0b1100`) code with no check: for the stream's last nibble that is past the `vector<bool>`'s size. MSVC keeps the bits past the size zero in the last word (`_Trim`), so there it reads 0; when the stream ends on a 32-bit word it reads the word past the vector's storage (heap memory, undefined) | `TriangleSelector.cpp`, `update_used_states`; the MSVC STL's `vector<bool>` | 0 everywhere. Only a corrupt painting string (its first character `C`) reaches it; MFAC records the in-word case and leaves out a string that would end on a word |
| `FacetsAnnotation::reset()` clears the triangles and the bit stream but not `used_states` | `Model.cpp:3535-3540` | kept (MFAC: reset, then a triangle set again) |
| `get_extruders` builds a `TriangleSelector` from the painting (`deserialize`) and lists each state whose triangles are not empty (`get_facets`). Each triangle's tree takes only the nibbles it needs, so a nibble past it (`48`: a leaf of state 2, then a 4) names no state, where `used_states` counts it; one triangle id at or past the mesh's facets and `deserialize` reads nothing. Its cache, `mmuseg_ts`, has no initializer; the reader touches the painting after loading it, so the first call builds the list | `Model.cpp:2536-2565`, `:3494-3499`; `TriangleSelector.cpp:1496-1520`, `:1772-1893` | the trees read as `deserialize` reads them, without building the splits (`facets.rs`, `leaf_states`), the cache left out. A split code whose special side does not fit (3 sides, side 1 to 3) indexes past the triangle's corners in the C++ (undefined); here the tree is read as any other, and no reader writes one. Probed (M3MF's Bambu fixture: `8` and `48` on a part of extruder 3, whose arrange polygon lists 2 and 3) |
| `deserialize`'s info line for a painted triangle id past the mesh ("array bound:error:triangle_id >= int(m_triangles.size())"), which `get_extruders` reaches when it builds its list: the C++ builds it once per timestamp of the painting (`mmuseg_ts`) | `Model.cpp:2544-2547`; `TriangleSelector.cpp:1781-1784`; `Format/bbs_3mf.cpp:5058-5069`, `:5216-5230` | `ModelVolume::get_extruders` keeps no `mmuseg_ts` cache, so a painted triangle id past the mesh logs `deserialize`'s info line at each call, where the C++ logs it once per timestamp (`Model.cpp:2544-2547`); the 3MF reader paints only a part's own triangles (`bbs_3mf.cpp:5058-5069`, `:5216-5230`), so no record reaches it |
| `.front()` of an empty result: the hull's expansion for the bed's decompositions (a hull of fewer than three points) and `get_shared_poly`'s intersection of extruder areas that do not overlap | `BuildVolume.cpp`, `PrintConfig.cpp`, `get_shared_poly` | an empty polygon; not probed (no bed of fewer than three points, overlapping extruder areas only) |
| `smallest_enclosing_circle_welzl` of no points leaves the squared radius unset | `Geometry/Circle.hpp:142-150` | a default circle (radius 0); MCIR records no empty input |
| `BuildVolume` reads `extruder_printable_heights[index]` for each extruder area, past the heights' end when there are fewer | `BuildVolume.cpp` | panics; the configuration gives one height per area |
| `circle_ransac` samples with a default-seeded `std::mt19937` and `std::sample` (selection sampling through `_Rng_from_urng`), whose draws the standard leaves to the library | `Geometry/Circle.cpp:111-138`; the MSVC STL's `algorithm` | MSVC's algorithm (`circle.rs`, which carries it); probed (MCIR, MBVL round beds) |
| `opt_float`, `opt_int` and `get_config_value<...>(...)->value` on a missing option dereference a null pointer (`get_real_skirt_dist`, `get_instance_arrange_poly`'s `enable_support` and `support_type`) | `PrintConfig.cpp`, `ModelArrange.cpp` | panics naming the option; the command line's configuration holds every option |
| A matrix whose linear part is not finite has no SVD (`computeRotationScaling`), where Eigen computes with an unset one | `Geometry.cpp:420-440` | NaN matrices; no reader gives such a matrix to the front end |
| Object ids and timestamps (`ObjectBase`, `ObjectWithTimestamp`) | `ObjectID.hpp` | not kept: the command line's loading never reads them. Open for phase O4 (the 3MF writer and the plate data use them) |
| The OBJ reader passes its material library's path to `mtlparse` and the log as `path.string().c_str()`: the `std::string` is a temporary that dies at the end of the statement, so the pointer is to freed memory. Under 16 bytes the path sits in the dead string's own buffer, which is empty by then: `mtlparse` opens "" and the load fails (`model-unreadable`, "load mtl in obj: failed to parse", the log line `load_obj:load_mtl: failed to parse ` with an empty path). At 16 bytes or more it is on the heap and the freed block still holds the path, for `mtlparse` and for the log line (`failed to parse <path>`, or `failed to load mtl_path:<path>` when the library is missing). The staged engine and the staged oracle give the same, two passes each (parity/CORPUS.md) | `Format/OBJ.cpp:56-65` | the path itself, whatever its length, for `mtlparse` and for the log lines alike (where the C++ logs an empty path under 16 bytes, the translation's line names the path); probed (MLOD: the probe's libraries are found through the OBJ file's folder, so their paths are 16 bytes or more, and the cube fixture's library and the probe's libraries beside the file load in the C++ as in the translation). The corpus holds `model-obj-mtl-info` and `-layers` (`painted.mtl`, 11 bytes, found by its bare name in the case's folder) as engine faults, and `model-obj-mtl-long-*` (19 bytes) and `model-obj-mtl-missing-*` to their families (parity/CORPUS.md) |
| A material library's name is tried as a path from the process's working folder before the OBJ file's folder (`boost::filesystem::exists(mtl_name)`), so what a run reads depends on where it was started | `Format/OBJ.cpp:43-55` | the same, with `std::path::Path::exists`; the probes use names that are not in either working folder |
| `set_face_color` reads a face's texture coordinates by index with no bound: a corner without a texture index (-1 after the adjustment) reads before the vector, one past the coordinates after it | `Format/OBJ.cpp:164-167` | such a coordinate reads as 0. Only `ObjInfo` (the GUI's colours) holds them, so the model does not change; the fixtures give the faces of a known material texture indices in range |
| A negative texture index is made absolute with the number of texture coordinates taken as `size() / 3`, where the vector holds two floats per coordinate | `Format/objparser.cpp:247-248` | kept; only `ObjInfo` reads it |
| An `o` line's name is copied after the parser has walked past it, so it is always empty | `Format/objparser.cpp:322-336` | kept; nothing reads the names |
| `mtl_parseline` keeps the current material in a `static std::string`, shared by every load in the process (two OBJ files read at once would mix their materials) and reset at each `mtlparse` | `Format/objparser.cpp:376`, `:616` | a value passed through one `mtlparse`; the command line reads one file at a time |
| The reader checks a line's length only after each 64 KiB read: an unfinished line of more than 65536 bytes fails the file ("load_obj: failed to parse"), so a line of up to about 128 KiB passes or fails by where it starts | `Format/objparser.cpp:568-600`, `:635` | kept; probed (MLOD: lines of 65536, 100000 and 140000 bytes in the file and its library) |

The 3MF reader (`load_bbs_3mf` under `LoadModel | AddDefaultInstances`), met while its records
were made (orca-3mf-probe, 2026-11-10). M3MF records each file's log (what stderr gets), then its
error or loaded model, and names the piece of the port it needs (piece 1: the container and the
root model; 2: the config entries; 3: the sub-model files and the assembly). Its hazards, each
for the piece that translates its lines (lines of `src/libslic3r/Format/bbs_3mf.cpp` unless
named). All three pieces are translated (orca-3mf-port-1, 2026-11-10, orca-3mf-port-2 and
orca-3mf-port-3, 2026-11-11, `crates/orca-model/src/bbs_3mf/`): each row is done as its last
column says, and every record of all three replays.

| What | Where | What the translation does |
| --- | --- | --- |
| Every failure the importer reports (`add_error`) goes only to the log: `load_bbs_3mf` calls `log_errors()` after the load, which logs each at error, and the command line prints records of error and fatal on stderr (`cli/main.cpp`, `log_to_stderr`, `[error] <text>`), where certify compares them. The exception says less: `read_from_file` throws "Loading of a model file failed." for every failure and "The supplied file couldn't be read because it's empty" for a file of no object. A file that loads can log errors too (3mf-3-colors, 3mf-2-numbers), one failure logs nothing (3mf-1-rels-no-model), the records the importer logs itself at error come before the `add_error` texts, and some texts are glued or carry the C++ function and line ("Unable to open the file<path>", "Slic3r::_BBS_3MF_Importer::_load_model_from_file:1803, loading sub-objects error", which ends with its own line feed) | `:731-739`, `:1803`, `:8962`, `Model.cpp:346-355` | the same texts in the same order, function names and line numbers written as the C++ prints them; M3MF records the log of every read (error and fatal, `ReadLog` in the probe) |
| The attribute getters: a float through the bundled `fast_float::from_chars` (no leading `+` or space, the digits it can read, 0 when it reads none), an int through Spirit's `qi::int_` (a sign, the digits it can read, 0 when it reads none), a bool through `atoi` (true when the attribute is missing). An int past `int`'s range keeps the digits read before the overflow: Spirit assigns its partial value and fails, which the getter ignores, so `2147483648` and `2147483649x` are 214748364. This fast_float has no range error: a float past `float`'s range is ±infinity, one below it 0; also `nan`, `inf` and `infinity` in any case with an optional `-`, and `nan(<chars>)` (parse_infnan); not probed, for the reason given for a float past float's range | `:500-520`; `deps_src/fast_float/fast_float.h:2407-2440`, `:2338-2376` | piece 1, as written; probed (3mf-3-numbers: `+1`, ` 2`, `0x1p3`, `3abc`, `1e-50`, `+0`, `1.9`, ` 1`, printable `2` and `x`, an object id `2147483648` that build items find as `214748364` and `0214748364`). A float past `float`'s range is not probed: floats reach only vertices (and text configurations, which M3MF does not record), and an infinite vertex would carry infinities through the hulls and the placement |
| The model's information: each metadata's text is unescaped by expat and then again by `xml_unescape` (`&amp;`, `&lt;` and `&gt;` only, in one pass), so `&amp;amp;` in the file is `&`; every named metadata goes into `metadata_items` by name; a `<metadata>` with no name or an empty one repeats the last name, as `m_curr_metadata_name` is never reset; `Copyright` and `CopyRight` both set the copyright, the later wins; the design information exists only when a Designer is given, the profile and model information always. The exporter writes it into a `.gcode.3mf` again | `:3917-4036`, `:1833-1851`, `:6790-6816`; `utils.cpp:1346-1380` | piece 1; probed (3mf-3-inch: Title, Designer, DesignerUserId, Description, Copyright, CopyRight, License, Origin, ProfileTitle, an unknown name, a nameless one and entities; 3mf-3-plain's Title; the full set's projects). `Model`'s `design_info`, `profile_info` and `model_info` (with `metadata_items`) hold it, and the replay writes them |
| A transform is 12 numbers split on spaces with `token_compress_on` (a space at either end adds an empty token), each read with `atof` (the C locale's `strtod`: hexadecimal, `inf`, leading spaces); any other count gives the identity. An offset to the assembly is 3 such numbers | `:593-644` | piece 1; probed (3mf-3-numbers: a short transform and a doubled space) |
| A build item whose transform has a zero scale keeps the identity (`_apply_transform` returns) | `:4241-4248` | piece 1; probed (3mf-3-numbers) |
| The model's unit (micron, centimeter, inch, foot, meter) scales each vertex as a `float` (`m_unit_factor * value`), not the build items' or components' transforms, and each sub-model file has its own unit | `:645-660`, `:3519`, `:3776-3780`, `:5318` | pieces 1 and 3; probed (3mf-3-inch, 3mf-3-units: a centimeter root and a meter sub-model) |
| `m_is_bbl_3mf` comes from the `Application` metadata (starts `BambuStudio-` or `OrcaSlicer-`) or from an `OrcaSlicer` metadata whose `Semver::parse` succeeds. It decides whether a broken `model_settings.config` fails the file and whether an object of several build items is split into one object per item | `:3946-3962`, `:1952-1958`, `:1998-2041` | piece 1 (with Semver, `deps_src/semver`); probed (3mf-3-orca-version, 3mf-3-orca-bad-version, 3mf-2-bbl-settings-*, 3mf-3-settings-*) |
| Names: at the root model's end a non-BBL file of one object names it after the file's stem; the split copies each object with the name it has then (empty, or that stem); only after it are the objects of the file named, from their own name or `Object_<index + 1>`, so the copies keep theirs | `:1732`, `:3545-3550`, `:2032`, `:2150-2155` | pieces 1 and 3; probed (3mf-3-plain: a copy named ""; 3mf-3-numbers: `Object_1` and copies named after the stem) |
| The sub-model files are read in a `tbb::parallel_for`, one `ObjectImporter` (which opens the archive again) each; their objects are merged in the importers' order, and an id already held keeps the first. The workers' `add_error` calls take the importer's lock, in the order the threads reach it | `:1746-1824`, `:731` | piece 3: `orca-tbb`'s `parallel_for` over the importers, each behind a lock of its own and the errors behind the importer's (`bbs_3mf/objects.rs`), so in a parallel arena the errors come in the order the workers reach the lock, as in the C++; the merge order is the relationships file's, an id already held keeping the first. The oracle's arena has one thread, which reads the files one after another in order, so the records hold that order; probed (3mf-3-submodel-*, 3mf-3-units) |
| `ObjectImporter::object_xml_parser` has no initializer, and when a sub-model fails the importers are never deleted (`return false` skips the merge that deletes them), so their parsers leak. The destructor, which tests the pointer, is reached only by importers that made their parser | `:933-1072`, `:1797-1816`, `:5715-5722` | piece 3: owned values, dropped (`bbs_3mf/objects.rs`); no output differs |
| `ObjectImporter::object_unit_factor` has no initializer: only the sub-model's `<model>` element sets it, so a vertex read before it (a file whose root element has another name) is scaled by whatever the heap held | `:933-1072`, `:5318` | piece 3: the factor starts at 1.0, millimetres (`bbs_3mf/objects.rs`). No record holds such a file, and none can hold the C++'s answer (the heap's bytes) |
| The handlers around `mz_zip_reader_extract_to_callback` never run: the build is `/EHsc`, which takes an `extern "C"` function (miniz) not to throw, so the `catch` blocks of `_extract_model_from_archive` and `ObjectImporter::_extract_object_from_archive` are left out. The callback's `FileIOError` is caught one frame up by `_extract_from_archive`, which adds `e.what()` alone, so a sub-model's parse error is logged without the " for <path>" its own handler would add, and the `version_error` rethrows are dead. The cause is read from the build's flags; the records show the effect (the error with no suffix) | `:2526-2549`, `:5747-5772`, `:2427-2438` | pieces 1 and 3: the error goes to `_extract_from_archive`'s handler, with no suffix (`bbs_3mf/mod.rs`, `bbs_3mf/objects.rs`); probed (3mf-3-submodel-bad-xml) |
| `std::stoi` throws through the importer and `read_from_file`, with MSVC's texts "invalid stoi argument" and "stoi argument out of range": in the brim ears' header, and in `model_settings.config`'s `other_layers_print_sequence_nums`, there from inside an expat callback (C frames, which MSVC's x64 unwinding crosses; the parser is not freed) | `:2965`, `:4435` | piece 2: the same texts; probed (3mf-2-brim-bad-version, 3mf-2-plate-bad-nums, 3mf-2-plate-nums-range) |
| The cut information and the height ranges are read with Boost 1.84's `property_tree::read_xml` and `get<T>` through `stream_translator` (`num_get`). Its exceptions escape `read_from_file`: `<unspecified file>(<line>): <rapidxml's message>`, `No such node (<path>)`, `conversion of data to type "<type>" failed` with the type as Boost names it ("unsigned long long" for `size_t`, "int", "float", "double") | `:2559-2616`, `:2886-2943`; Boost's `property_tree/` | piece 2: the same texts; probed (`ptree.sop1` PTXR and PTGT hold each text: every rapidxml message flags 0 can reach, `No such node (<path>)` for a missing node, an empty key and a missing `objects`, and the conversion text of each of the four types; 3mf-2-ranges-bad-xml fails at line 8, the end of the data, not at its wrong closing tag; M3MF also holds 3mf-2-cut-*, 3mf-2-ranges-* and 3mf-2-numbers' variants). orca-xml's `read_xml` and `Ptree` give them (orca-ptree-port, 2026-11-11) |
| `read_xml` parses with rapidxml, a second XML parser beside expat, with flags 0 (`parse<parse_comment_nodes>`): comments kept as `<xmlcomment>` children, the declaration, doctypes and instructions skipped, white space neither trimmed nor normalised (the white space between children joins the parent's data), the five entities and character references translated and any other `&` kept as written, data and CDATA appended to the parent's data, attributes under `<xmlattr>` in order (a repeated one kept twice; `get` finds the first), CDATA at the top level in the root's data, a top-level comment a child of the root. It does not check that a closing tag names the open element (`</b>` and `</>` close `<a>`). An element name is anything but white space, `/`, `>`, `?` and NUL (so `<`, `=`, `"` and `!` are name characters: `<a!>` is an element `a!`), an attribute name anything but those and `<`, `=` and `!`; attributes need no space between them. The UTF-8 byte order mark is skipped only at the very start, and a NUL at the top level ends the document. A character reference is a 32-bit `unsigned long` that wraps (`&#4294967361;` is `A`); a decimal one takes `a`-`f` and `A`-`F` as the digits 10 to 15 (`&#1a;` is U+0014); one of no digits (`&#;`, `&#x;`) is 0 and writes a NUL into the data, and `&#X41;` (an upper-case `X` is no digit) is 0 followed by "expected ;"; one at U+110000 or past is "invalid numeric character entity"; surrogates are written as three bytes; a missing `;` is "expected ;". The error's line counts the line feeds from the start of the buffer to where rapidxml stopped, in the buffer as rapidxml has rewritten it by then: the NUL that ends a closed element's or an attribute's name overwrites the character after the name (a line feed there is lost), text and attribute values expanded in place leave their old bytes behind the new end (their line feeds count twice), `&#10;` adds a line, an invalid reference is counted to where its character would have been written, and "expected ;" to where it was read | Boost's `property_tree/detail/rapidxml.hpp` (its parser, `:1343-2596`), `xml_parser_read_rapidxml.hpp` | piece 2 (orca-ptree-port, 2026-11-11: `orca-xml`'s `rapidxml`, which rewrites its buffer as rapidxml does, statement by statement, and `read_xml`); probed (`ptree.sop1` PTXR, `parity/oracle/probe/ptree.cpp`: 70 trees, 69 failures with every message flags 0 can reach from each place that throws it, 15 of them on the line count, and in the full set 30 seeded mutations of each; `crates/orca-xml/tests/ptree.rs` replays all 139 committed and 4249 full-set records with no difference). M3MF reaches it too, through the cut information and height ranges of 10 committed records and 26 more in the full set |
| Under flags 0 rapidxml never throws "expected ?>", "expected PI target", "invalid closing tag name" or "expected attribute name" (its test follows a step that has already taken a name character), nor the end of data of a skipped comment or of `parse_no_data_nodes`; `read_xml`'s "read error" needs a stream that fails, which a `std::istringstream` does not | `rapidxml.hpp:1760`, `:1777`, `:1888`, `:1901`, `:2003`, `:2199`, `:2247`; `xml_parser_read_rapidxml.hpp` | piece 2: those branches are not translated (`orca-xml`'s `rapidxml` and `ptree` say so); no record holds them |
| rapidxml's parser and `read_xml_node` recurse once per element level, so a deep enough cut information or height ranges file overflows the stack (undefined; a crash on Windows) | `rapidxml.hpp` (`parse_element` and `parse_node_contents`), `xml_parser_read_rapidxml.hpp` (`read_xml_node`) | piece 2: the port keeps its own stack, so no input can overflow it (orca-xml-stack, 2026-11-11): `orca-xml`'s `rapidxml` keeps the open elements on a `Vec` and finishes each at its closing tag as `parse_element` would, every write and error in the C++'s order; `read_xml` copies the nodes into the tree over a stack, and a `Ptree` is cloned, compared, printed and dropped over stacks. A file deep enough to overflow the C++ engine's stack crashes the engine, and the port reads it instead; no record can hold a crash. `ptree.sop1` holds 200 levels; `orca-xml`'s tests read, clone, drop and replay 1,000,000 levels on a 256 KiB stack, and two failing documents of 1,000,000 levels give the message and line the recursive reader gave |
| `get<T>` takes what `num_get` reads and nothing after it but white space (`stream_translator`: `>> value`, then `>> std::ws` when not at the end, then the end of the data); the white space is the classic locale's (space, `\t`, `\n`, `\v`, `\f`, `\r`; not U+00A0). `int` is read as a `long` (32 bits) by `_Stolx`, `size_t` as an `unsigned long long` by `_Stoullx`, which takes a sign and wraps a negative number (`-1` is 18446744073709551615, `-18446744073709551615` is 1, `-0` is 0); both are base 10, so `0x10` is refused, and leading zeros never count. `float` and `double` go through `strtof` and `strtod` on the field `num_get` collects: a sign, a decimal or `0x` hexadecimal significand (`0x10` is 16, `0x1p-1` 0.5), an exponent of at least one digit (`1e`, `1e+` and `5e` are refused) whose value is held to 1100 (`0e99999999999999999999` is 0, `1e2147483648` is refused), at most 768 significant digits, a dropped digit that is not 0 raising a last kept 0 or 5 by one (so a tie written past 768 digits rounds up). A result past the type's range, or one that rounds to 0 (`1e-46` and `0x1p-150` for `float`, `2.4703282292062327e-324` for `double`), is a range error and refused; a denormal is taken (`4.9e-324`, and `7.1e-46` for `float`). Refused too: empty or blank, `inf`, `nan`, `1.#INF`, a comma, text or a NUL after the number, a second number, a space after the sign, two signs, `.`. With a default (`get<int>("<xmlattr>.id", -1)`) a refused form or a missing node gives the default and nothing throws, so an id past `int`'s range is "Found invalid object id"; without one the failure throws `conversion of data to type "<type>" failed` | `:2575`, `:2593-2606`, `:2902`, `:2920-2921`; MSVC 14.42's `xlocnum` (`do_get` for `long`, `unsigned long long`, `float`, `double`) | piece 2: orca-fmt's `istream` reads `long` and `unsigned long long` through one `_Parse_int_with_locale` (`get_i32` with `_Stolx` and its `_Stoulx`, orca-xml-stack; `get_u64` with `_Stoullx`), `float` (`get_f32`) and `double`, and orca-xml's `Ptree::get` and `get_or` read through it as `stream_translator` does (orca-ptree-port, 2026-11-11). Probed (`ptree.sop1` PTGT: 273 texts in 15 groups under all four types, `DBL_MAX` and the rounding boundary past it in full among them, numbers as an element's text, defaults, and the reader's calls on 3mf-2-numbers and each of its 25 variants, every record of both sets replayed with no difference; M3MF holds 3mf-2-numbers and its variants too: `volume_id="2147483648"` and `r_tolerance="1e39"` committed, the rest in the full set) |
| `set_deserialize` of a height range's option, or of an object's or part's metadata in the assembly, throws "Invalid value provided for parameter <key>: <value>" out of `read_from_file`; an unknown key is dropped (substitutions `EnableSilent`) | `:2932`, `:2131`, `:5116`, `:5264` | pieces 2 and 3: the same; probed (3mf-2-ranges-bad-option, 3mf-3-bad-value, 3mf-3-unknown-key) |
| Other numbers: `boost::lexical_cast<int>` in `parse_int_list` (the whole token, a sign and digits, within `int`; it throws `bad_lexical_cast` for any other token, which `parse_int_list` catches and drops, `:553-556`), `std::istringstream >> std::boolalpha` for the plates' flags (`true` or `false` as a prefix, case and all, else false and the stream failed; a blank value leaves the flag as it was), `stringstream >> int` for the plates' sequences (numbers until the first that fails), `atoi` and `atof` for the metadata, layer heights and brim ears | `:541-562`, `:4359-4367`, `:4404-4610`, `:2824-3013`; Boost 1.84's `lexical_cast` (`shr_signed`, `lcast_ret_unsigned`), MSVC 14.42's `xlocnum` and `xlocale` (`num_get::do_get` for `bool`, `_Getloctxt`) | piece 2, each as its library reads (`bbs_3mf/lexical_cast.rs`; orca-fmt's `get_bool_alpha` and `get_i32`); the plates' flags and lists do not reach the model the command line keeps, and are not probed |
| The brim ears of version 0 are read four numbers at a time with no check that four remain: a count that is not a multiple of four reads past the vector's end (undefined) | `:3002-3006` | piece 2: the last group of fewer than four is dropped, the groups before it kept; nothing is read past the list. No record holds such a line, and none can hold the C++'s answer (the bytes past the vector's end). Ruled 2026-11-12 (orca-o2-review): the drop stays; an error would throw away the points the C++ keeps, and Orca's writer (`:7584-7600`) always writes groups of four. |
| A component that refers back to its own object, directly or not, makes `_generate_current_object_list` loop for ever, its list growing | `:4867-4892` | piece 3: refused before the walk (a depth-first search of the components the object reaches): the error "found a cycle in the components of object <id>" is added and the load fails, "Loading of a model file failed." (`bbs_3mf/assemble.rs`, `components_reach_a_cycle`). No record holds a cycle, and none can hold the C++'s answer (it never ends) |
| A part made from a mesh another part holds (`add_volume_with_shared_mesh`, the `mesh_shared` metadata) shares its `shared_ptr<const TriangleMesh>`, and `center_geometry_after_creation` moves the mesh in place through a `const_cast`, so centring one part would move every part that shares the mesh | `:4955-4972`, `:5023-5036`; `Model.cpp:1295-1306`, `:2600` (`ModelVolume::center_geometry_after_creation`) | piece 3: the mesh is an `Arc`; centring copies a shared one first (`Arc::make_mut`, `model.rs`), so only that part moves. The reader centres only a part it has just made from its own mesh, which nothing shares yet, so the two never differ there. Probed through the split (3mf-3-plain, 3mf-3-numbers: the parts of the object a copy was split from share the copy's meshes). No record holds the `mesh_shared` metadata, and none centres a shared mesh. |
| `load_model_from_file` asks `Model::get_backup_path()`, which makes `<temporary_dir>/orcaslicer_model/<local day and time>#<pid>#<id>/` with `3D/Objects`, `Metadata` and `lock.txt` (class P: the time, the process) | `:1406-1417`; `Model.cpp:959-999` | piece 1: the same folders. Nothing recorded reads the path |
| The `<id>` in the backup folder's name is the model's `ObjectID`, which counts every `ObjectBase` the process has made before it (objects, parts, instances, configurations) | `Model.cpp:971`, `ObjectID.hpp` | piece 1: a counter of the models that ask (`NEXT_MODEL_ID`, `model.rs`), as object ids are not kept (class P either way). The info logs print `this` as 16 hexadecimal digits, the Rust model's address. Nothing recorded reads the path or those logs |
| `get_backup_path` removes a folder of the same name left from before with `boost::filesystem::remove_all`, which throws on a failure (outside the `try`); a failure to make the folders is logged as "Failed to create backup path" with the quoted path and `filesystem_error`'s text | `Model.cpp:975-996` | piece 1: a failed removal is ignored (the name holds the process id and a fresh id, so no run reaches it), and the logged text after the path is Rust's I/O error's, not Boost's. Neither is recorded |
| `~Model` queues the backup folder's removal as a task for the backup manager's thread (`remove_backup`, `_BBS_Backup_Manager`), which removes it after the model is gone | `Model.cpp:176-183`; `:8652-8677`, `:9037-9040` | not translated: the folder stays under the temporary folder. A caller sets that folder with `utils::set_temporary_dir(work_dir)` first, as `cli/run.cpp:240` does; without it the folder is made at `/orcaslicer_model` at the drive's root |
| The backup id a BBL file's object UUID gives at a build item (an object UUID ending in `OBJECT_UUID_SUFFIX`, read as hexadecimal by `std::istringstream >> std::hex`) goes to `Model::set_object_backup_id` | `:4177-4183` | piece 1: not kept. Only the GUI's backup and restore read it again; nothing the command line runs does, as object ids are not kept. Probed only through what it does not change (no record writes it) |
| `add_error` texts made with `snprintf` into 1024-byte buffers are cut at 1023 bytes. Cut inside a UTF-8 character, the C++ keeps the bytes before the cut | `:2423`, `:2484`, `:2532` | piece 1: cut at 1023 bytes; a cut inside a character loses the rest of that character as `U+FFFD`, since the log takes a Rust string. An entry name that is not UTF-8 (one found through `encode_path`, in the local code page) is written with `U+FFFD` in the parse error's text the same way. No recorded path is that long, and every recorded entry name is ASCII. Piece 2: a height range's option key or value that is not UTF-8 is passed to `set_deserialize` with `U+FFFD` (rapidxml does not check the bytes); no record holds one |
| A float attribute's value: fast_float computes the correctly rounded `float` with its fast path, Eisel-Lemire and its long-decimal path, and stops adding exponent digits once the exponent reaches 0x10000 | `deps_src/fast_float/fast_float.h:597`, its `decimal_to_binary` and `parse_long_mantissa` (`:2310`, `:2441`) | piece 1: the grammar is fast_float's (`bbs_3mf/fast_float.rs`); the value is Rust's correctly rounded `f32` parse of the same characters. They differ only for an exponent past about 655359 balanced by as many mantissa digits, which no file holds; the probed numbers match |
| `encode_path` converts a UTF-8 entry name to the ANSI code page with `boost::nowide::widen` and `WideCharToMultiByte(CP_ACP)`, so the second lookup's bytes depend on the machine's code page (class P) | `utils.cpp:1143-1162` | piece 1: the same calls through FFI on Windows (`utils.rs`), the bytes unchanged elsewhere. The probe's entry names are ASCII, where every code page agrees |
| An entry is looked up by its name as UTF-8, then through `encode_path` (the ANSI code page, class P), then by the Info-ZIP Unicode path extra field of each entry | `:2381-2403`, `utils.cpp:1143` | piece 1, in that order; the probe's entry names are ASCII, where the three agree |
| The probe's archives are packed by `model.cpp`'s own writer (tdefl level 6, a fixed date), not miniz's writer, which stamps the local time | `parity/oracle/probe/model.cpp` (`pack_zip`) | the fixtures' bytes are in the records; nothing is packed in the replay; marker files beside a fixture (wrong-crc, claimed-size, stored, duplicate) pack an entry with a CRC one bit off, a claimed uncompressed size (for a stored entry the compressed size too, `end` or `past` set from the archive's size; an empty entry keeps compressed size 0), stored rather than deflated, or a second entry of the same name |
| An entry whose extraction fails (a stored CRC that does not match its data, a deflate stream miniz refuses) fails only after its whole data was read: miniz checks the CRC at the end. A reader of piece 2 that reads an entry into memory then logs its own text and goes on: the svg shape (3187), brim ears (2950), cut information (2565), height ranges (2892), layer heights (2830). `_extract_xml_from_archive` logs "Error while reading config data to buffer" (2478) for every entry it reads: the relationships fail the file (1742-1743), `model_settings.config` fails a BBL file (1952-1958) and is passed over in another, and the sub-model relationships' result is ignored (1758). A model read through the callback has been parsed whole, its objects made, before the CRC fails it: the root model logs "Error while extracting model data from zip archive" (2552), a sub-model the same text with " for <path>" (5775) | `:2476-2479`, `:2559-2567`, `:2824-2832`, `:2886-2894`, `:2944-2952`, `:3182-3189`, `:2538-2554`, `:5759-5777`; miniz `mz_zip_reader_extract_to_mem_no_alloc` (`deps_src/miniz/miniz.c:4441-4445`, `:4518-4521`), `mz_zip_reader_extract_to_callback` (`:4763-4788`) | pieces 1, 2 and 3: the same texts after the same reading; probed (3mf-1-rels-bad-crc, 3mf-2-bad-crc: every entry piece 2 reads, in entry order; 3mf-3-submodel-bad-crc; the broken archive "the root model's CRC one bit off") |
| `_extract_xml_from_archive` asks expat for a buffer of `(int)m_uncomp_size` bytes, the size the entry's central directory claims, before anything is extracted. A claim whose `int` is negative, or above 2^30, makes `XML_GetBuffer` return NULL without allocating (its size doubles from 1024 and overflows): "Unable to create buffer" (2472). A smaller claim allocates the power of two at or above it (from 2048 bytes, up to 1 GiB for a claim of 2^30); a claim larger than the data then fails the extraction (2478). The readers of piece 2 and the svg shape allocate a `std::string` of the claimed size before extracting | `:2470-2479`, `:2562`, `:2827`, `:2889`, `:2947`, `:3184`; expat `XML_GetBuffer` (`src/deps/EXPAT/expat/xmlparse.c:2043-2125`) | piece 1: test the claimed size as `XML_GetBuffer` does, before any allocation, and never allocate from a claim; piece 2: never allocate from a claim; a claim larger than the data fails the extraction and logs the reader's own text, as the C++ does (a claim the C++ cannot allocate is not probed). Both through `bbs_3mf/entries.rs`'s `extract_claimed`: miniz's checks run first (a directory or an entry with no compressed data succeeds without writing; encryption, another method, or a claim below the entry's own size fails); a stored entry's own claim is held to the archive's size, and a deflated entry is inflated once with its bytes counted and kept nowhere, so only data that inflates to its claim is given a buffer, of that size; the string's NULs past it are counted, not made. Probed (3mf-1-rels-size: the relationships claim 2^31 bytes; orca-3mf-probe-3: a deflated over-claim by index and by name, a stored over-claim inside the archive and to its end, and two entries of one name that the by-name extraction finds as a shorter, a longer and an empty entry: 3mf-2-claim-heights-over, 3mf-2-claim-cut-over, 3mf-2-claim-heights-stored-over, 3mf-2-claim-heights-stored-end, 3mf-2-dup-cut-longer-first, 3mf-2-dup-cut-longer-second, 3mf-2-dup-cut-empty-second). miniz refuses at open a stored entry whose claim passes the archive by more than its name and an entry of no compressed data that claims bytes (3mf-1-claim-heights-stored-past, 3mf-1-claim-heights-no-data). A claim of 0xFFFFFFFF, which skips that check, is not probed (a 4 GiB string) |
| A sub-model entry of no bytes is refused before its parser is made: "Found invalid size for <path>". The texts that end "for <path>" (5718, 5372, 5422, 5775, 1006) carry the path as the relationships' Target gives it, with its leading `/`; the parse error's quoted name (5753) is the archive's own entry name, without it | `:5715-5720`, `:5369-5430`, `:5753`, `:1006` | piece 3: the same texts; probed (3mf-3-submodel-empty, 3mf-3-submodel-invalid-object, 3mf-3-submodel-duplicate-object, 3mf-3-submodel-bad-crc) |
| `model_settings.config`'s handlers keep one current object, part and plate, not a stack. A `<plate>` inside a plate replaces the open one (which is never freed); the inner `</plate>` stores it and clears the current plate, so the outer `</plate>` (4662) or the `</model_instance>` around the inner plate (4686) finds none, as does a `<model_instance>` outside a plate (4674). An element outside an object finds object id -1 (4077, 4106, 4318, 4391, 4755); inside an object before its first part, no part (4081, 4111, 4322, 4759). Each of these handlers returns false, which stops the parser: "Error (parsing aborted) while parsing xml file at line <n>" follows, a BBL file is refused ("Archive does not contain a valid model config") and another goes on with what was read before the stop. A `<text_info>` in a part that has a text configuration logs "Both text_info and text_configuration found, ignore legacy text_info" (4766) and goes on | `:3440-3515`, `:4073-4134`, `:4263-4400`, `:4649-4702`, `:4751-4768`, `:1664-1672`, `:1952-1959` | piece 2: one current object, part and plate, as the C++ has; probed (the 3mf-2 fixtures of orca-3mf-errors: text, shape, mesh_stat and text_info each with no object and with no part, text-info-twice, plate-nested, instance-no-plate, instance-nested-plate) |
| An assembly item's `object_id` is looked up among every object the root model and the sub-models define (`m_index_paths`, built at the root model's end), then among the objects a build item made (`m_objects`): "can not find object for assemble item, id= <id>" for either miss (4724, 4730) | `:4715-4744`, `:3541-3544` | piece 2: the same two lookups; probed (3mf-2-assemble-no-object, 3mf-2-assemble-unbuilt: a component no build item builds) |
| The plates are kept by their `plater_id` and checked with `it->first > m_plater_data.size()`, an `int` against a `size_t`: an index past the number of plates, or a negative one (converted to a large unsigned), is "invalid plate index" (2278), and so are indexes that skip a number (plates 1 and 3); an index of 0 passes and writes `plate_data_list[-1]` (undefined). Only a file that gets past the assembly reaches the check | `:2266-2306`, `:4404-4407`, `:4658-4667` | piece 3: refuse as the C++ does. An index of 0 passes the check as in the C++, and the port never writes out of bounds: it keeps no plate list (the command line's load releases it unread, `cli/run.cpp:256-273`), so nothing is written for that plate, and its instances take their loaded ids from it as any plate's do (2313-2348); piece 2 keeps the plate by its index as written (`bbs_3mf/config.rs`, `handle_end_config_plater`, which says so). No record can hold the C++'s answer: it writes through the pointer stored before the list's heap block, whose outcome is undefined (a crash, or memory overwritten elsewhere). Probed (3mf-3-plate-index: one plate of index 2). No record holds an index of 0 |
| Texts no file reaches, translated as written with no record: a failed allocation (2460, 2503, 5724: `XML_ParserCreate`; 4160, 4169, 4202: `add_object` and `add_instance` throw rather than return null); a branch only Restore takes (3649, 5405); a re-open of the file the importer holds open (999) and the parse error a sub-model's own handler sets, which the same callback always turns into a thrown error (1018); checks the lines before them make false: 3533, 2004 and 2069 (an object's index is the model's size when it is made and no object is removed before), 2011 (`m_objects` gets only ids `m_current_objects` holds, and only Restore erases one), 4142 (the only call passes 1), 4897 (each object is built once, and the split's copies are made before any part), 4911 (`sub_objects` come from the same map) and 4977 (`sub_objects` take only objects with triangles) | the lines named | all pieces: as written |
| A plate's members other than its index, its instances and its four thumbnail names go only into the plate list (`plate_data_list`), which the command line's load releases unread: `locked`, `plate_name`, the plate's `config` (`curr_bed_type`, `print_sequence`, `first_layer_print_sequence`, `other_layers_print_sequence`, `other_layers_print_sequence_nums`, `spiral_mode`, `filament_map_mode`, `filament_map`, `enable_filament_dynamic_map`, `has_filament_switcher`), `filament_maps`, `gcode_file`, `pattern_bbox_file`, `gcode_prediction`, `gcode_weight`, `toolpath_outside`, `is_support_used`, `is_label_object_enabled`, `printer_model_id`, `nozzle_diameters`, `slice_filaments_info` (each `<filament>`'s id, type, colour, use in metres and grams, tray id, use for objects and supports, nozzle group ids, nozzle diameter and volume type) and `warnings` (each `<warning>`'s message and level) | `:4404-4576`, `:4588-4641`, `:2266-2306`; `cli/run.cpp:256-273` | piece 2: not stored (`bbs_3mf/config.rs` names each). Each is still parsed as the C++ parses it (`atoi`, `atof`, `xml_unescape`, `boolalpha`, `get_vector_from_string`, `parse_int_list`, the enum's name looked up, `std::stoi`), then dropped, so what throws still throws; stored: `plate_index`, `obj_inst_map`, `thumbnail_file`, `no_light_thumbnail_file`, `top_file` and `pick_file`, which the assembly reads (the instances' loaded ids and its logs). Probed through what throws (3mf-2-plate-bad-nums, 3mf-2-plate-nums-range) and the plates' instances (piece 3) |
| `<mesh_stat>` and `<text_info>` test the current part with `(volumes.size() - 1) < volume_id` in `size_t`: an object with no parts passes it (the size less one wraps), and its empty list is indexed (a null pointer: the engine dies). That happens when an `<object>` opens inside a part of another, the part's id kept | `:4321-4326`, `:4758-4763` | piece 2: refused as a part past the list is ("can not find part for mesh_stats", "can not find part for text_info"), which stops the parser. No record holds it, and none can hold the C++'s answer: the oracle dies there |
| `CutObjectBase(ObjectID(get<size_t>("<xmlattr>.id")), get<size_t>("<xmlattr>.check_sum"), get<size_t>("<xmlattr>.connectors_cnt"))`: C++ leaves the order of a call's arguments unspecified, so which of two failing attributes throws first is the compiler's (a connector's braced list is read in order) | `:2593-2595`, `:2603-2606` | piece 2: read as MSVC does, right to left: `connectors_cnt`, then `check_sum`, then `id`. Probed (orca-3mf-probe-3: 3mf-2-cut-order-id-sum, -id-cnt, -sum-cnt with two attributes missing, and -id-bad-cnt, -bad-sum-cnt, -id-bad-sum with one missing and one not a number) |
| Past the bytes the extraction writes, expat's buffer is uninitialized: a claim larger than its entry's own (two entries of one name, the second's stat with the first found by name) or an entry with no compressed data (which miniz takes as extracted with nothing written) leaves the rest of the claim unwritten, and the parse reads it. A `std::string` reader sees NULs there | `:2470-2482`; miniz `mz_zip_reader_extract_to_mem_no_alloc` | piece 2: one NUL stands for the unwritten bytes, which ends the parse with an error at that byte. No record holds such an entry read into expat's buffer, and none can hold the C++'s answer (the heap's bytes). The `std::string` readers' NULs are probed (3mf-2-dup-cut-longer-first, 3mf-2-dup-cut-empty-second) |

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
