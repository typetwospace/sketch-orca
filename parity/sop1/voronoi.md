# voronoi.sop1

The record kinds of the voronoi probe (`parity/oracle/probe/voronoi.cpp` and the sources of
`parity/oracle/probe/voronoi/`, phase O3): Boost.Polygon's Voronoi builder, libslic3r's Voronoi
utilities, the repair and the medial axis, recorded by orca-o3-probe-voronoi (2026-11-13) from the
oracle's build (Boost 1.84 under `<work>/deps`, libslic3r under `<work>/src`). The values are those
of `parity/SOP1.md` ("The container", "Values" and "Probe files"). Every kind starts with `VR`, and
no other page defines it (`scripts/lib/parity/probe.ts`, `probeKindProblems`, which
`test/parity-probe.test.ts` runs). `orca-voronoi` replays the records, piece by piece:

| piece | C++ it carries | kinds |
| ----- | -------------- | ----- |
| voronoi-1 | `voronoi_ctypes.hpp`, `voronoi_robust_fpt.hpp`, `voronoi_predicates.hpp` 1-524 | VRUL, VREF, VRXC, VRXI, VRRF, VRRD, VRSQ, VRCP, VRPC, VREC, VRDP, VRNC, VRCE |
| voronoi-2 | `voronoi_predicates.hpp` 525-1533, `voronoi_geometry_type.hpp`, `voronoi_structures.hpp`, `voronoi_builder.hpp` | VRMP, VRLZ, VRCF, VRQU, VRSS, VRBT |
| voronoi-3 | `voronoi_diagram.hpp`, `voronoi.hpp`, `Geometry/Voronoi.hpp`, `PolygonsPointIndex.hpp`, `PolygonsSegmentIndex.hpp`, `VoronoiUtils.hpp`, `VoronoiUtils.cpp` | VRVD, VRVG, VRUT, VRPI, VRCR, VRDZ |
| voronoi-4 | `Geometry/Voronoi.cpp`, `VoronoiUtilsCgal.hpp`, `VoronoiUtilsCgal.cpp`, `MedialAxis.hpp`, `MedialAxis.cpp`, `ExPolygon.cpp` 261-378 | VRGD, VRCG, VRAN, VRMA |

A record holds its inputs, then what the engine computed from them, so the replay needs no
generator. Each section names the piece that replays it; the crate's probe test rebuilds every record
of its pieces from the Rust and compares it whole.

## Inputs

Nothing comes from the corpus. The inputs are built by `parity/oracle/probe/voronoi/common.cpp`,
`parity/oracle/probe/voronoi/issues.cpp` and `parity/oracle/probe/voronoi/search.cpp`:

- **Site sets** (`SiteSet`, the builder's own input: points, then segments, in 32-bit coordinates):
  hand-made sets (empty, one point, one segment, two points, vertical and horizontal collinear points,
  grids, duplicate points, the point sets of the "points 12067" and "division by zero 12903" cases of
  OrcaSlicer's `tests/libslic3r/test_voronoi.cpp`, a zero-length segment, chains, a triangle,
  reversed, vertical and collinear segments, spokes, nested squares, points inside a square, and
  coordinates at ±2147483000 and ±2147483647); then the contours of every shape of at most 400
  segments, as segments; then seeded sets (seed `0x5EED5E7`): point clouds, grids with holes,
  collinear runs with an outlier, and star polygons with a hole or points inside.
- **Shapes** (ExPolygons in scaled coordinates, 1 = 1 nm): hand-made squares, holes (one turned
  clockwise), a triangle, a sliver, an L, collinear and repeated points, holes and slits two
  nanometres wide (as the SPE-1729 shapes `MedialAxis::build` names); the issue polygons of
  `test_voronoi.cpp` (missing vertex 1, 2 and 3, duplicate vertices, intersecting edges, gapfill
  12707 turned as its test turns it, weirdness polygons 2, 5 and 7); slices of the OBJ meshes of
  OrcaSlicer's tests (`<exe>/../src/tests/data`, read by the probe's own OBJ reader: `v` lines and
  `f` lines fanned from their first vertex) at 13 %, 50 % and 77 % of their height through
  `slice_mesh_ex` with default parameters on one thread; and seeded stars (seed `0x57A125`), half
  with a clockwise hole.
- **Arachne's polygons** of a shape: its contours and holes with consecutive repeated points and
  collinear points removed (`remove_collinear`) and polygons under three points dropped, the kind of
  input the engine's Arachne gives SkeletalTrapezoidation.
- **Thin cases** (an ExPolygon with a minimum and a maximum width, the medial axis' input):
  hand-made strips of 0.3, 0.1, 1.2 mm and 3 nm, a wedge, L, T and cross strips, square and round
  rings, a zigzag, a tapered strip, a strip with a nanometre hole and a strip with narrow widths;
  the thin-wall regions of every shape as the classic walls make them (`offset2_ex(region, -(w/2 +
  s/2 - 1), s/2 - 1)`, then `opening_ex(diff_ex(region, offset(that, w/2 + ClipperSafetyOffset)),
  min/2)`, widths nozzle/3 to w + s) and gap-fill regions as the walls and the fill make them
  (`diff_ex(opening_ex(gaps, min/2), offset2_ex(gaps, -max/2, max/2 + ClipperSafetyOffset))`, min
  0.2 w (1 - 0.15), max 2 s), for flows w/s/nozzle of 0.42/0.38/0.4 and 0.65/0.59/0.6 mm; then
  seeded thin rings (seed `0x7E1A9`).
- **Line sets** (`parity/oracle/probe/voronoi/issues.cpp`, VRGD only): segments `test_voronoi.cpp`
  gives as lines, copied as given: lines0 to lines4 of "Alessandro gapfill 12707" (17, 14, 13, 9 and
  8 lines) and the 958 lines of "Voronoi NaN coordinates 12139" (Boost ticket 12139; the test says
  they intersect themselves, and Boost 1.84's builder still makes 23 vertices with NaN coordinates
  from them).
- **The repair search** (`parity/oracle/probe/voronoi/search.cpp`, VRGD only): 20,000 draws, draw i
  from its own generator `Gen(0x5EA4C8 + i)`, each one ExPolygon of a family: 0 a square (10 µm to
  50 mm, along the axes half the time, else turned) with one to three vertices off its edges at a
  seeded point, moved an integer −5 to 5 nm along the edge's normal (0 leaves it on the edge, as the
  missing-vertex polygons of `test_voronoi.cpp` have it), a quarter of them with a square hole that
  has up to two; 1 a star of 5 to 24 points with one or two such vertices; 2 a square or a star with
  one vertex replaced by two 1 to 3 nm apart; 3 a sliver 1 to 20 nm wide (a triangle or a
  quadrilateral); 4 a square with a slit 1 to 10 nm wide or a notch of that opening; 5 a square with
  a triangular hole whose apex lies 1 to 5 nm inside its edge. A draw whose edges intersect
  (EdgeGrid's `intersecting_edges`, as `test_voronoi.cpp` checks its own inputs) is skipped. Each
  draw is constructed with the repair on; VRGD keeps every draw whose detection finds an issue. For
  each rotation of such a draw's repair whose detection found an issue other than a missing vertex,
  VRGD also keeps the draw **turned** by that rotation's angle with `Point::rotated`, as the repair
  turns its input: the builder then gets that rotation's segments, so the turned draw's own
  detection finds what the rotation found (the probe checks it), and the first detection reaches the
  issues the draws themselves never give it.

The coordinates of the issue polygons, point sets and line sets are OrcaSlicer's
(`test_voronoi.cpp`, v2.4.2).

## voronoi.sop1's values

Besides the values of `parity/SOP1.md` (`u8` to `u64`, `i32`, `i64`, `f32` and `f64` by their bits,
little-endian; `bool` a `u8` 0 or 1; `str` a `u32` byte count and the bytes):

- a point: `i64` x, `i64` y; points: a `u32` count, then each point; polygons: a `u32` count, then
  each polygon's points; an ExPolygon: its contour's points, then its holes as polygons; ExPolygons:
  a `u32` count, then each; lines: a `u32` count, then each line's point a and point b;
- a site set: a `u32` count of points, each `i32` x, `i32` y; then a `u32` count of segments, each
  `i32` x1, y1, x2, y2;
- a site (`site_event<int32>`): `i32` x0, y0, x1, y1 (equal points for a point site), `u64` sorted
  index, `u64` initial index, `u8` source category (0 single point, 1 segment start point, 2
  segment end point, 8 initial segment, 9 reverse segment), `bool` inverse. An inverse site holds
  its points swapped, as `site_event::inverse` leaves them;
- a site reference (VRBT): `u32` sorted index, `bool` inverse;
- a circle (`circle_event<double>`): `f64` x, `f64` y, `f64` lower x, `bool` active;
- a big integer (`extended_int<64>`): `i32` count (its sign the number's, its size the number of
  chunks in use), then |count| `u32` chunks, lowest first. Chunks past the count are not recorded:
  the C++ leaves them as they were;
- an efpt (`extended_exponent_fpt<double>`): `f64` val, `i32` exp, as the object holds them (val in
  [0.5, 1) or 0, the number val · 2^exp);
- a robust fpt: `f64` fpv, `f64` re; a robust dif: its positive sum, then its negative sum, each a
  robust fpt;
- an error (an exception a call threw): `u8` 1 `Slic3r::InvalidArgument`, 2 `Slic3r::OutOfRange`, 3
  `Slic3r::RuntimeError`, 9 any other, then `str` what();
- a log: `str`, every Boost.Log record the calls wrote, each `[<severity>] <message>` and a line
  feed;
- a diagram (`voronoi_diagram<double>` or `Slic3r::Geometry::VoronoiDiagram` through its
  accessors): a `u32` count of cells, each `u64` source index, `u8` source category, `i32` incident
  edge, `u32` colour; a `u32` count of vertices, each `f64` x, `f64` y, `i32` incident edge, `u32`
  colour; a `u32` count of edges, each `i32` cell, `i32` vertex0, `i32` twin, `i32` next, `i32`
  prev, `u8` flags (1 linear, 2 primary), `u32` colour. An index is the element's position in its
  container, -1 for none; colours are `color()`, the stored value shifted down by 5. A vertex
  coordinate that is NaN is written as `0x7ff8000000000000`, whatever bits the CPU made: a replay
  compares a coordinate by NaN-ness when either side is NaN (a NaN matches any NaN), and by its bits
  otherwise. Degenerate input makes such vertices (VRGD: the 12139 lines and three failed repairs);
- the issue (`VoronoiDiagram::IssueType`): 0 no issue, 1 finite edge with a non-finite vertex, 2
  missing Voronoi vertex, 3 non-planar diagram, 4 edge intersecting an input segment, 5 parabolic
  edge without a focus point, 6 unknown; the state (`VoronoiDiagram::State`): 0 repair not needed,
  1 repair successful, 2 repair unsuccessful, 3 unknown; a state block: `u8` issue, `u8` state,
  `bool` is_valid, `u64` num_vertices, num_edges, num_cells.

The committed set (`parity/probes/voronoi.sop1`) is a true subset of the full set
(`<work>/parity/probes/voronoi.sop1`) in the full set's order: per kind, the hand-made records of at
most a byte limit and the first others up to a count (`Budget` in `common.hpp`), and for the
captured kinds of the builder runs every n-th capture up to a count (`write_captured` in
`builder.cpp`), and for VRGD the smallest record of each repair outcome (VRGD below). Arithmetic
cases (voronoi-1) whose results would hold a NaN are left out: the C++ makes the CPU's default NaN
there, which a port need not reproduce. Diagrams keep their NaN vertices under the rule above.
Infinities stay.

## The pieces and their edges

What a port must copy exactly, because the records depend on it:

- **Coordinates are narrowed to 32 bits.** `default_voronoi_builder` is `voronoi_builder<int32>`,
  and `voronoi.hpp`'s `insert` passes `coord_t` (64-bit) coordinates to `insert_point` and
  `insert_segment`, which take `int32`: a coordinate outside the 32-bit range keeps its low 32 bits.
  Every input here is inside the range.
- **The site order is MSVC's `std::sort`.** `init_sites_queue` sorts the sites with
  `event_comparison_predicate`, then drops equal sites with `std::unique` (equal points, whatever
  their index or category). Equal sites are common: every polygon vertex is the end point of one
  segment and the start point of the next. Which one survives decides the cell's source index and
  source category, and that is where `std::sort` puts it: MSVC's introsort (insertion sort up to 32
  elements, a median-of-three or ninther pivot and a three-way partition above, heap sort when the
  depth budget runs out). voronoi-2 replays that algorithm; VRSS records it on every site set.
- **Tied circle events pop in MSVC's heap order.** The circle event queue (`ordered_queue`) is a
  `std::priority_queue` over list iterators, ordered by `event_comparison_predicate` reversed; MSVC's
  `push_heap` and `pop_heap` decide the order of events that compare equal. VRQU records scripts
  with ties, and VRBT shows the order inside the builder.
- **The beach line is a `std::map` with hinted inserts.** The node comparison is only meaningful for
  the pairs the builder compares, so a port keeps MSVC's insert-with-hint path (the hint taken when
  it fits, else a search from the root) or a structure that gives the same order on the same calls.
  VRNC holds sampled comparisons, VRBT the result.
- **The release build defines `NDEBUG`.** No assert runs: `get_source_point` does not check the
  index it reads, `compute_point_cell_range` walks every edge of the cell without checking they are
  finite (the probe calls it only where they are, as Arachne's input gives it), and the repair
  leaves an invalid cell range alone.
- **`site_event`'s constructors leave the initial index unset**; the builder sets it in
  `insert_point` and `insert_segment` (points and segments share one counter), and every site the
  probe makes sets it.
- **Arithmetic shifts.** `extended_exponent_fpt::sqrt` halves a negative exponent with `>>`, which
  MSVC makes arithmetic (rounding toward minus infinity).
- **`extended_int` keeps unnormalized values.** Subtraction checks only the top chunk for zero, so
  a difference can keep leading zero chunks in its count (VRXI holds one); comparisons and the
  vector constructor take the count as it is. Sums and products stop at 64 chunks and drop the
  carry.
- **The repair rounds with `Point::rotated`**: `round` (half away from zero) of the rotated `double`
  coordinates, for the angles π/6, π/5, π/7 and π/11 in that order, and rotates the other vertices
  back with `make_rotated_vertex`; a vertex at an input segment's end is put back on that end
  exactly. `discretize_parabola` rounds the same way.
- **The repair is a loop of rotations with one detection each.** `construct_voronoi` writes the
  warning of the issue it detected, then tries the four angles in turn, each a fresh construction of
  the turned segments copied into the object (`copy_to_local`) and detected again, and stops at the
  first that finds no issue; when none does, the object keeps the last rotation's diagram, the issue
  that rotation found and `REPAIR_UNSUCCESSFUL`, and the error names that issue. VRGD lists every
  rotation run and what it found, with repairs that succeed at the second, third and fourth angle and
  rotations that find a non-finite vertex or an intersecting edge.
- **A second construction after a failed repair reads a stale diagram.** After
  `REPAIR_UNSUCCESSFUL` the diagram keeps `m_is_modified` true, so when `MedialAxis::build` constructs
  again from the morphological closing, the detection and, if it finds no issue, the accessors read
  the copies of the last rotation, not the new diagram. The detection's cell pass looks up each stale
  segment cell's source index in the new lines, and `get_source_segment` throws `OutOfRange` when
  that index is past them (seven VRGD records). The planarity check's `get_source_point` reads
  without a check under `NDEBUG`, but it runs only after that pass has looked up every
  non-degenerate segment cell without a throw, and a point cell's index is that of a segment, so it
  reads inside the new lines whenever each segment has a non-degenerate cell (the closing's lines
  have no zero-length segment).
- **`ExPolygon::medial_axis` takes its maximum width with `fmaxf`**: the widths are rounded to
  `float` before the comparison, and the short-polyline test compares with that rounded value.
- **VoronoiUtilsCgal's predicates are exact.** The orientations come from CGAL's filtered predicates
  (an interval filter, then exact `MP_Float`), so `orca-exact` must give the exact sign. With a
  collinear focus side, CGAL's `perpendicular` (its precondition off) returns the clockwise vector.
- **`annotate_inside_outside` lives in `VoronoiOffset.cpp`**, which no piece carries; voronoi-4
  translates it as a helper of the medial axis (VRAN).

The edges where a port stops with NotPorted:

- the `ColoredLines` instantiations of `construct_voronoi`, `get_source_segment`,
  `get_source_point`, `compute_segment_cell_range` and `is_voronoi_diagram_planar_angle`: only
  `MultiMaterialSegmentation.cpp` (painted facets, outside domain A) uses them;
- `is_voronoi_diagram_planar_intersection`: no domain-A caller reaches it (Arachne calls it only
  under `ARACHNE_DEBUG`). VRCG records its answers so a port can add it; until then it is NotPorted;
- coordinates outside the 32-bit range (above), and `to_point` on a vertex outside the 64-bit range
  (`llround` overflows; no domain-A caller passes one).

## voronoi-1: arithmetic and predicates

### VRUL

`ulp_comparison<double>` (voronoi-1). `f64` a, `f64` b, `u32` maxUlps, then `i32` the result (-1
LESS, 0 EQUAL, 1 MORE). Hand-made: every pair of 16 special values (zeros of both signs, ±1, values
one ulp from 1, the smallest subnormal, the smallest normal, ±DBL_MAX, ±infinity and NaNs of both
signs) at 0 and 64 ulps. Seeded: a double, and one up to 300 ulps from it or another double.
Piece: voronoi-1.

### VREF

`extended_exponent_fpt<double>` (voronoi-1). `f64` va, `i32` ea, `f64` vb, `i32` eb; then the efpts
A = efpt(va, ea), B = efpt(vb, eb), efpt(va), −A, A + B, A − B, A · B; `bool` B nonzero and A / B
when it is; `bool` A not negative and A.sqrt() when it is; `f64` A.d(), `f64` B.d(); `bool`
A.is_pos(), is_neg(), is_zero(). The compound operators are assignments of these (`*this = *this +
that`). Cases exercise the exponent gap of 54 at its edges, zeros with an exponent, subnormal values
and cancellation; results whose val is not finite are left out. Piece: voronoi-1.

### VRXC

`extended_int<64>`'s constructors (voronoi-1). `u8` form: 0 then `i32` v (from `int32`), 1 then `i64`
v (from `int64`), 2 then `bool` plus, a `u32` count and the `u32` chunks highest first (from a
vector); then the big integer. The vector constructor keeps the lowest 64 chunks of a longer vector
and leading zero chunks as they are. `INT32_MIN` and `INT64_MIN`, whose negation overflows in the
C++, are not inputs. Piece: voronoi-1.

### VRXI

`extended_int<64>`'s operations (voronoi-1). Big integers x and y; `u8` bits of x == y (1), != (2),
< (4), > (8), <= (16), >= (32); the big integers x + y, x − y, x · y, −x; `i32` m and x · m; `i64`
m64 and x · m64; `f64` and `i32` of x.p(), `f64` x.d(); the efpt of `type_converter_efpt` and the
`f64` of `type_converter_fpt`. Operands are normalized (top chunk not zero) with chunks often 0, 1
or 0xFFFFFFFF; hand-made cases hold the unnormalized difference and 64-chunk overflow.
Piece: voronoi-1.

### VRRF

`robust_fpt<double>` (voronoi-1). Robust fpts x and y; `bool` y.fpv nonzero, `bool` x.fpv not
negative; then robust fpts x + y, x − y, x · y, x / y (when y.fpv is nonzero), the same through
`+=`, `-=`, `*=` and `/=` (`/=` when nonzero), −x, x.sqrt() (when x.fpv is not negative); `bool`
has_pos_value, has_neg_value, has_zero_value of x. Ties of sign and equal magnitudes (whose relative
error divides by a zero sum and gives infinity) are among the inputs. Piece: voronoi-1.

### VRRD

`robust_dif<robust_fpt<double>>` (voronoi-1). Robust difs d1 and d2 (non-negative sums), robust fpt
v, `bool` v.fpv nonzero, the robust fpt d1.dif(); then robust difs −d1, d1 + d2, d1 + v, v + d1,
d1 − d2, d1 − v, v − d1, d1 · d2, d1 · v, v · d1, d1 / v (when v is nonzero), and d1 after `+= v`,
`+= d2`, `-= v`, `-= d2`, `*= v`, `*= d2`, `/= v` (when nonzero). Piece: voronoi-1.

### VRSQ

`robust_sqrt_expr<extended_int<64>, efpt, type_converter_efpt>` (voronoi-1): eval1 to eval4, the sum
of A[i] · sqrt(B[i]). `u8` n (1 to 4), n big integers A, n big integers B (not negative), then the
efpt result, each from a fresh object. A third of the seeded cases cancel their first two terms
exactly. Piece: voronoi-1.

### VRCP

`robust_cross_product` and `orientation_test` (voronoi-1). `u8` form 0: `i64` a1, b1, a2, b2 (each
inside ±(2^32 − 1), the range of a difference of two `int32`), `f64` the cross product, `i32` the
orientation of the four-value `eval` (-1 right, 0 collinear, 1 left); form 1: three points as `i32`
x, y each, then `i32` the orientation of the three-point `eval`. Sums of two products past 2^64 wrap
as the C++'s `uint64` does. Piece: voronoi-1.

### VRPC

`point_comparison_predicate<point_2d<int32>>` (voronoi-1). `i32` x1, y1, x2, y2, then `bool` the
answer. Piece: voronoi-1.

### VREC

`event_comparison_predicate<site_event<int32>, circle_event<double>>` (voronoi-1). `u8` form: 0 site
and site, 1 site and circle, 2 circle and site, 3 circle and circle; the two operands; `bool` the
answer. Seeded operands (sites in small ranges, vertical segments, inverse sites, circles within 150
ulps of the site's x) come first, then the comparisons the builder made (`builder.cpp`, captured:
the sort of the sites, the main loop's site-against-circle test and the queue's circle
comparisons). Piece: voronoi-1.

### VRDP

`distance_predicate<site_event<int32>>` (voronoi-1). Left site, right site, `i32` x, `i32` y of the
new point, then `bool` the answer. These are the calls the builder's node comparisons make (for each
captured VRNC pair whose comparison points differ in x: the node whose newer site's comparison point
lies left, against the other's point; `voronoi_predicates.hpp` 389-404). Piece: voronoi-1.

### VRNC

`node_comparison_predicate<beach_line_node_key<site_event<int32>>>` (voronoi-1), as the beach line's
`std::map` calls it in the builder runs. Node 1's left and right sites, node 2's left and right
sites, then `bool` the answer. A sample: every comparison of a hand-made site set up to 40 per run,
else every 7th up to 8. Piece: voronoi-1.

### VRCE

`circle_existence_predicate<site_event<int32>>` (voronoi-1), as `circle_formation_predicate` calls
it in the builder runs. `u8` form (0 ppp, 1 pps, 2 pss, 3 sss), `i32` the segment or point index
it passes (0 for ppp and sss), the three sites in the order it passes them, `bool` the answer.
Piece: voronoi-1.

## voronoi-2: circle formation and the builder

### VRMP

`mp_circle_formation_functor` (voronoi-2), on the triples the lazy functor met in the builder runs
(VRLZ's inputs). `u8` form (0 ppp, 1 pps, 2 pss, 3 sss), `i32` index, the three sites, `u8` flags
(1 recompute c_x, 2 recompute c_y, 4 recompute lower_x), the circle before (the lazy functor's
answer), the circle after. Each triple comes twice: with all three flags, then with one of the six
other sets in turn. Piece: voronoi-2.

### VRLZ

`lazy_circle_formation_functor` (voronoi-2), as `circle_formation_predicate` calls it in the builder
runs. `u8` form, `i32` index, the three sites as passed, then the circle it made (its recomputation
through the mp functor included). Piece: voronoi-2.

### VRCF

`circle_formation_predicate` (voronoi-2), as the builder calls it in `activate_circle_event`. The
three sites, `bool` whether the formation functor ran (the existence predicate let it), `bool` the
answer (false also when `lies_outside_vertical_segment` drops the circle), then the circle when the
functor ran. Piece: voronoi-2.

### VRQU

`ordered_queue<std::pair<circle_event<double>, uint32>, cmp>` (voronoi-2), with the builder's order
(`voronoi_builder.hpp` 160-165: the comparison of the right event before the left).
`u32` the number of operations, then each: `u8` 0 push, the circle and `u32` its id; `u8` 2
deactivate, `u32` the id (through the reference `push` returned); `u8` 1 pop, then what the top was
before the pop: `u32` id, `bool` active. Hand-made scripts and half the seeded ones push circles of
lower x in {0, 1, 2} and y in {0, 1}, so ties are many. Piece: voronoi-2.

### VRSS

The builder's site order (voronoi-2): the site array as `insert_point` and `insert_segment` make it
from a site set (`voronoi_builder.hpp` 59-95), sorted with `std::sort` and
`event_comparison_predicate`, `std::unique`d and given its sorted indexes (`init_sites_queue`). The
site set, then a `u32` count and each site of the result. Piece: voronoi-2.

### VRBT

The builder's run (voronoi-2): every call `voronoi_builder::construct` makes on its output, for one
site set. The site set; the sorted sites as the calls show them: a `u32` count, each `bool` seen and,
when seen, `i32` x0, y0, x1, y1 (as sorted, not inverse), `u64` initial index, `u8` category; then a
`u32` count of calls, each `u8` tag: 0 `_reserve`, `u64` the site count; 1 `_process_single_site`,
the site reference; 2 `_insert_new_edge(site1, site2)`, two site references, `u32` the indexes of
the two new half-edges; 3 `_insert_new_edge(site1, site3, circle, edge12, edge23)`, two site
references, `f64` circle x, y and lower x, `u32` edge12, edge23 and the two new half-edges; 4
`_build`. Edge indexes are positions in the diagram's edge vector at the call (before `_build`
compacts it). The diagram these calls build is VRVD's for the same site set; the probe checks the two
are equal. Piece: voronoi-2.

## voronoi-3: diagrams and utilities

### VRVD

`boost::polygon::construct_voronoi` into `voronoi_diagram<double>` (voronoi-3; the builder is
voronoi-2's). `u8` entry: 0 points only, 1 segments only, 2 points and segments (points inserted
first); the site set; the diagram after `_build`. Piece: voronoi-3.

### VRVG

`Slic3r::Geometry::VoronoiDiagram`'s point and mixed overloads of `construct_voronoi` (voronoi-3),
on the site sets with points and at most 60 sites. `u8` entry (0 points, 2 points and lines), the
site set, the state block (always unknown: these overloads never repair), the diagram.
Piece: voronoi-3.

### VRUT

VoronoiUtils' vertex helpers (voronoi-3). `f64` x, y, `f64` angle, `u32` colour; `bool` is_finite;
`bool` is_in_range<int64_t> of x, of y, is_in_range<int32_t> of x, of y, is_in_range<int64_t> and
<int32_t> of the vertex; `bool` safe (finite and inside ±9·10^18), then when safe `i64` x, y of
`to_point(vertex)` and of `to_point(&vertex)`; when finite, `make_rotated_vertex(vertex, angle)`'s
`f64` x, y and `u32` colour. Piece: voronoi-3.

### VRPI

Arachne's `PolygonsPointIndex` and `PolygonsSegmentIndex` (voronoi-3). `u8` form 0: the empty index's
point and `bool` initialized. Form 1: Arachne's polygons of a shape (at most 400 points), a `u32`
count, then for every point index in order: `u32` polygon, `u32` point, p(), `u32` next's and
prev's point index, the segment index's from() and to(), `PolygonsPointIndexSegmentLocator`'s two
points, `PathsPointIndexLocator`'s point, `u64` `std::hash` (PointHash), then `u32` polygon and
point of a seeded other index and `bool` <, ==, != against it. Piece: voronoi-3.

### VRCR

The source and cell-range lookups on every cell of a raw diagram (voronoi-3), built by
`VoronoiDiagram::construct_voronoi(..., false)` (no repair, so it is the builder's diagram). `u8`
source: 0 the lines of a shape (`Lines::iterator`), 1 Arachne's polygons of a shape as
`PolygonsSegmentIndex` (their const_iterator), 2 a hand-made site set of points and segments.

For sources 0 and 1 (at most 800 segments): the lines or the polygons; a `u32` count of the
non-degenerate cells, each `u32` cell index, `bool` contains segment, `bool` contains point; a
segment cell then holds `get_source_segment` (source 0 its two points, source 1 `u32` polygon and
point index) and `compute_segment_cell_range` (start point, end point, `i32` edge begin, `i32` edge
end, `bool` valid); a point cell holds `get_source_point`, and for source 1 also
`get_source_point_index` (`u32` polygon, point) and `bool` called, then when called
`compute_point_cell_range` (source point, `i32` edge begin, end, `bool` valid). It is called when
the cell's first edge is infinite or out of range (it returns at once) or when every edge of the cell
is finite. Then the throwing calls: a `u32` count, each `u8` call (0 `get_source_segment`, 1
`get_source_point`, 2 `get_source_point_index`), `u32` cell, `u64` range length, and `u8` 0 when it
returned or the error: the wrong-kind calls on the first segment cell and the first point cell
(`InvalidArgument`), and `get_source_segment` with the range cut before the cell's index
(`OutOfRange`).

For source 2 (`Slic3r::VoronoiDiagram`'s mixed overload): the points, the lines, a `u32` count, then
for each non-degenerate cell `u8` 1 for a point cell (`get_source_point`) or 0 (`get_source_segment`
on the lines), `u32` cell, `u64` line count, then `u8` 0 and the point or the line's two points, or
the error: point cells hold `SOURCE_CATEGORY_SINGLE_POINT` (`RuntimeError`), and segment cells source
indexes counted after the points (`OutOfRange` past the lines). A segment end point cell's index counts
the points too, and `get_source_point` reads the line there without a check: inside the lines it
returns that (wrong) line's end, past them the read is undefined, so it is not made and the cell holds
`u8` 255. Piece: voronoi-3.

### VRDZ

`VoronoiUtils::discretize_parabola` with a `PolygonsSegmentIndex` (voronoi-3). The source point, the
segment's from and to, start, end, `i64` the step, `f32` the transitioning angle, the points it
returns, the log (its warning when it would need an apex). First the primary parabolic edges of
Arachne's polygons' raw diagrams (up to 24 a shape), as SkeletalTrapezoidation's `discretize` calls
it (start and end from `to_point` of the edge's vertices), with steps 800000 and 50000 and angles of
10°, 30°, 1° and 0 as `float`; then seeded calls with the step a fraction of the span.
Piece: voronoi-3.

## voronoi-4: repair, planarity and the medial axis

### VRGD

`Slic3r::Geometry::VoronoiDiagram::construct_voronoi` on segments (voronoi-4): its detection, the
repair by rotation and the second construction `MedialAxis::build` makes after a failed repair.

- `u8` source: 0 the lines of a shape, 1 Arachne's polygons as `PolygonsSegmentIndex`, 2 a line set,
  3 the lines of a search draw; for source 3 also `u32` the draw, `u8` its family (0 to 5, as
  "Inputs" lists them) and `u8` turn (0 the draw as drawn, k the draw turned by the k-th angle);
- `bool` repair, then the lines (sources 0, 2 and 3) or the polygons (source 1), then the state
  block;
- when the repair is on, the rotations: `u8` how many `construct_voronoi` ran, then for each `u8`
  the issue its detection found, for π/6, π/5, π/7 and π/11 in turn up to the first that found none
  (0 rotations when the first detection found no issue). The probe gets them by calling the public
  `try_to_repair_degenerated_voronoi_diagram_by_rotation` itself, angle by angle on a fresh object,
  and fails unless the last one leaves byte for byte the diagram and the issue `construct_voronoi`
  left;
- `bool` has diagram, then the diagram as the accessors give it: the repaired one when the repair
  ran (state 1 or 2), and a line set's builder diagram when the repair is off (no VRVD record holds
  it);
- the log: the detection's warning, then the error of a failed repair, as `construct_voronoi` writes
  them;
- `bool` second: for one ExPolygon (sources 0 and 3) whose repair failed, `MedialAxis::build`'s
  second construction on the same object from `to_lines(closing_ex({expolygon}, 2 ·
  SCALED_EPSILON))`: those lines, then `u8` 0 with the state block and the diagram the accessors then
  return (stale, above), or `u8` 1 with the error; then its log.

Shapes come with the repair on and off (at most 800 segments), Arachne's polygons with the repair on
as Arachne constructs, the line sets with the repair on and, in the full set only, off, and the
search's draws and turned draws with the repair on.

What the 929 records of the full set reach (811 with the repair on, 118 with it off):

- the first detection: no issue in 216 (103 shapes, 113 Arachne polygons); a missing vertex in 527
  (9 shapes, the five 12707 line sets, 513 draws); a non-planar diagram in 51 draws; an edge
  intersecting an input segment in 9 (8 draws, 1 turned draw); a finite edge with a non-finite
  vertex in 8 (the 12139 lines and 7 turned draws);
- the rotations: besides the missing vertex, rotations find a non-finite vertex 15 times and an
  intersecting edge 4 times; repairs succeed after one rotation in 465 records, two in 21, three in
  23 and four in 9;
- the state left: `REPAIR_NOT_NEEDED` 216, `REPAIR_SUCCESSFUL` 518, `REPAIR_UNSUCCESSFUL` 77 and
  `UNKNOWN` 118 (the repair off). The failed repairs' errors: a missing vertex even after the
  rotation in 72 (the 12139 lines among them), a non-finite vertex in 3 and an intersecting edge in 2;
- the second construction, in the 76 failed repairs of one ExPolygon: it returns with
  `REPAIR_SUCCESSFUL` in 59 and `REPAIR_UNSUCCESSFUL` in 10, and throws `OutOfRange` ("Voronoi cell
  source index is out of range!") in 7;
- NaN vertices: the 12139 builder diagram (repair off) holds 23, and the three repairs left with a
  non-finite vertex hold one each.

Not reached, by any input here: a parabolic edge without a focus point, at the first detection or in
a rotation; a non-planar diagram after the rotations (no rotation's detection finds one); and the
"unknown issue" warning and error, which no detection can return. Of the 20,000 draws 572 detect an
issue: a missing vertex in families 0, 2 and 3; the slits and notches (family 4) give the non-planar
diagrams and the intersecting edges; the stars (family 1) and the holes by the contour (family 5)
detect none. No draw as drawn detects a non-finite vertex: those first detections come from the
turned draws and the 12139 lines.

The committed set holds 63 records: the shapes' and Arachne polygons' records as before (a hand-made
shape's with a diagram or the repair off, every hand-made Arachne one, and the first four others),
the 12707 lines4 set with the repair on, and of the search the
smallest record of each outcome (the issue first detected, the number of rotations, the issue and
state left, the second construction's result) of at most 9000 bytes. So it holds each detection
reached (a missing vertex 15 times, a non-planar diagram once, an intersecting edge twice, a
non-finite vertex three times), repairs that succeed after one, two, three and four rotations, each
error after the rotations reached (a missing vertex 4 times, a non-finite vertex once, an
intersecting edge once), a diagram with a NaN vertex, and second constructions that return repaired
(4), return unsuccessful (1) and throw (1). Three outcomes are in the full set only, because each of
their records passes 9000 bytes: the 12139 lines (non-finite first, then four rotations that find a
missing vertex; about 430 KB), draw 10951 (a missing vertex, then a non-finite vertex at the fourth
rotation, left non-finite; 10,876 bytes) and draw 6065 turned by π/5 (a non-finite vertex, left with
an intersecting edge; 11,443 bytes). Piece: voronoi-4.

### VRCG

VoronoiUtilsCgal's planarity checks (voronoi-4) on raw diagrams. `u8` source (0 a shape's lines, 1
Arachne's polygons, 2 a hand-made or seeded point set of 3 to 60 points, its diagram from the point
overload); the lines, polygons or points; then `is_voronoi_diagram_planar_angle` and
`is_voronoi_diagram_planar_intersection`, each `u8` 0 false, 1 true, or 2 and the error. Few answers
are false: of the 336 records of the full set, `is_voronoi_diagram_planar_angle` answers false in 3
and true in 333, `is_voronoi_diagram_planar_intersection` false in 3 and true in 333; of the 88
committed, the angle check answers false in 2 and true in 86, the intersection check false in 1 and
true in 87; neither throws. The angle check also runs inside the repair's detection: VRGD's 51
records whose first detection warns of a non-planar diagram are false answers of it on the builder's
diagram. Piece: voronoi-4.

### VRAN

`Slic3r::Voronoi::annotate_inside_outside` (voronoi-4, translated from `VoronoiOffset.cpp`) on the
diagram `MedialAxis::build` makes: `construct_voronoi` with the repair on, from a shape's lines (at
most 2000). The lines, the state block, then the categories it leaves: a `u32` count and `u8`
colour for every cell (0 boundary, 1 inside, 2 outside, 3 unknown), every vertex (0 on contour, 1
inside, 2 outside, 3 unknown) and every edge (0 points to contour, 1 inside, 2 outside, 3 unknown),
in container order; then the log. No record holds this diagram for most of these shapes: the
replay builds it from the record's lines with the repaired construction (voronoi-2 to voronoi-4,
`construct_voronoi` with the repair on) and annotates that. VRGD holds the same diagram only for the
shapes of at most 800 lines whose repair replaced the builder's diagram (state 1 or 2), and VRVD
holds the builder's own only for the contours of at most 400 segments. Piece: voronoi-4.

### VRMA

The medial axis (voronoi-4). The ExPolygon, `f64` min width, `f64` max width, then `u8` 0 and:
`MedialAxis::build`'s thick polylines, `ExPolygon::medial_axis`'s thick polylines, `bool` whether
`MedialAxis::build(Polylines*)` gave the same points as the thick build, `bool` whether
`ExPolygon::medial_axis(Polylines*)` gave the same points; or `u8` 1 and the error; then the log of
the four calls. Thick polylines: a `u32` count, each its points, a `u32` count of widths and each
`f64` (two per segment), `bool` endpoints.first, `bool` endpoints.second. Piece: voronoi-4.
