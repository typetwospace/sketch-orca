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
calls on 2026-11-06; phase O2's crates replay their own files from 2026-11-07), **pinned** (an oracle knob), **open** (nothing yet; the task that reaches the
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
| `deps_src/clipper/clipper.cpp:319` and `src/libslic3r/Int128.hpp:62-244` | Clipper's `sign_determinant_2x2_filtered` and `Int128::multiply`: MSVC takes `_mul128` | `i128` (orca-geom) |
| `src/libslic3r/Point.cpp:252-257` | 128-bit products | `i128` (orca-geom) |
| `Geometry/ConvexHull.cpp`'s `rotcalip::cmp_angles` | `boost::multiprecision::int128_t` and `int256_t` products | exact integers (orca-geom, with orca-exact's `int128_t` where Boost's signed magnitude decides a result) |
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
| Quaternion products by Eigen's SSE2 kernels, in an order of their own for each type | `Geometry/arch/Geometry_SIMD.h` | probed: `rotation.rs` `QuatProduct` |
| `sin`, `cos`, `atan2` and `pow` in `AngleAxis`, `Quaternion` and `eulerAngles` | `Geometry/AngleAxis.h`, `Geometry/Quaternion.h`, `Geometry/EulerAngles.h` | probed: the static UCRT's, through `orca-crt` (its FMA3 choice is the process's, as in the engine) |
| `JacobiSVD` of a matrix holding a NaN or an infinity leaves U, V and the singular values uninitialised | `SVD/JacobiSVD.h` | open: undefined in C++, so `JacobiSvd3::compute` and `compute_rotation_scaling` return `None` and the caller decides |
| NaN payloads when both operands are NaN: SSE2 returns the first operand's, and MSVC may swap the operands of a commutative operation | every floating-point operation | open: the probes use only the default quiet NaN (`0xFFF8000000000000`); a payload shows only in a printed NaN's sign |

### orca-geom

Clipper 6.4.2, `ClipperUtils` and libslic3r's geometry (points, lines, polygons, polylines,
ExPolygons, bounding boxes, convex hulls, `Geometry::Transformation`). Probe file `geom.sop1`.
Filled by o2-geom.

### orca-exact

Boost 1.84.0's `boost::multiprecision::int128_t` (signed magnitude, unchecked) and
`boost::rational`. Probe file `arrange.sop1`. Filled by o2-arrange.

### orca-qhull

Qhull 2015.2 (`libqhull_r` 7.2.0 and `libqhullcpp`), with its `qsort` calls through `orca-crt`'s
`qsort`, the static UCRT's own. Probe file `qhull.sop1`. Filled by o2-mesh.

### orca-mesh

admesh and `TriangleMesh`. Probe files `mesh.sop1` and, for the C runtime's stdio that admesh
reads with (`orca-fmt`'s `stdio` and `scanf`), `cstdio.sop1`. Filled by o2-mesh.

### orca-zip

miniz, as the 3MF reader and writer use it through `miniz_extension.cpp`. Probe file `zip.sop1`.
Filled by o2-model.

### orca-xml

expat 2.5.0, the dependency build's. Probe file `xml.sop1`. Filled by o2-model.

### orca-placeholder

The placeholder parser, for the presets' compatibility conditions. Probe file `placeholder.sop1`.
Filled by o2-front.

### orca-arrange

libnest2d, `Arrange.cpp`, NLopt 2.5.0's subplex and Nelder-Mead, and Boost.Geometry's R*-tree.
Probe file `arrange.sop1`. Filled by o2-arrange.

### orca-model

The STL, OBJ and 3MF readers, `Model`, `BuildVolume` and `ModelArrange`. Probe file `model.sop1`.
Filled by o2-model.

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
