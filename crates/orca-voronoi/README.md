# orca-voronoi

Phase O3 of the Rust translation: Boost.Polygon 1.84's Voronoi diagram of points and segments (its
robust arithmetic, predicates, structures and builder) and libslic3r's Voronoi utilities, polygon
indexes and medial axis.

Set up empty by orca-o3-skeleton on 2026-11-13: a manifest that declares every dependency the crate
will need, so that no unit that fills it changes `Cargo.toml` or `Cargo.lock`, and a `src/lib.rs`
with the header and the crate's doc comment. Each piece below adds its modules and its tests and
keeps this README true.

## The pieces that fill it

| Piece | What it adds |
| --- | --- |
| `orca-o3-voronoi-1` | Boost.Polygon's Voronoi types, robust arithmetic and first predicates (done, 2026-11-13) |
| `orca-o3-voronoi-2` | The remaining predicates, structures and the builder (done, 2026-11-13) |
| `orca-o3-voronoi-3` | The Voronoi diagram, VoronoiUtils and the polygon indexes (done, 2026-11-14) |
| `orca-o3-voronoi-4` | The construction and repair, the exact checks and the medial axis (done, 2026-11-14) |

The C++ they translate (under `src/libslic3r/` of OrcaSlicer v2.4.2 unless named): boost/polygon's
voronoi_ctypes.hpp, voronoi_robust_fpt.hpp, voronoi_predicates.hpp, voronoi_geometry_type.hpp,
voronoi_structures.hpp, voronoi_builder.hpp, voronoi_diagram.hpp and voronoi.hpp;
Geometry/Voronoi.hpp and .cpp, Geometry/VoronoiUtils.hpp and .cpp, Geometry/VoronoiUtilsCgal.hpp and
.cpp, Geometry/MedialAxis.hpp and .cpp, Arachne/utils/PolygonsPointIndex.hpp and
PolygonsSegmentIndex.hpp, ExPolygon.cpp; and the parts of Geometry/VoronoiOffset.hpp and .cpp that
MedialAxis calls, which no piece carries.

## The modules

Boost.Polygon's headers are read from the oracle's Boost 1.84
(`<work>/deps/OrcaSlicer_dep/usr/local/include/boost-1_84/boost/polygon/detail/`). Only
`voronoi_ctype_traits<int32>` is instantiated in OrcaSlicer (the default `voronoi_builder<int32>`),
so the modules compute with its types: `int32` coordinates, `int64` differences, `extended_int<64>`
and `extended_exponent_fpt<double>`.

| Module | Piece | C++ | What it carries |
| --- | --- | --- | --- |
| `ctypes` | voronoi-1 | `voronoi_ctypes.hpp` 1-643 | `ulp_comparison<double>`; `extended_exponent_fpt<double>` (`Efpt64`), whose `frexp` and `ldexp` are the static UCRT's (`orca-crt`); `extended_int<N>` (`BigInt` is `extended_int<64>`) with the C++'s unnormalised differences and its carry dropped at N chunks; the two converters; `voronoi_ctype_traits<int32>` as type aliases; the `Sign` and `GetSqrt` traits that stand for the free `is_pos`, `is_neg`, `is_zero` and `get_sqrt` overloads |
| `robust_fpt` | voronoi-1 | `voronoi_robust_fpt.hpp` 1-501 | `robust_fpt<double>` (`RobustFpt`), each operator and compound operator in its own rounding order; `robust_dif<robust_fpt<double>>` (`RobustDif`) with the free operators; `robust_sqrt_expr` over `extended_int<64>` and `Efpt64` (`RobustSqrtExpr`) with its scratch arrays |
| `predicates` | voronoi-1 | `voronoi_predicates.hpp` 1-524 | `robust_cross_product` (the `uint64` products and their sum, which wraps past 2^64 as the C++'s does), `orientation_test`, the point, event and node comparison predicates, `distance_predicate` and `circle_existence_predicate`; generic over the traits `VoronoiPoint`, `VoronoiSite`, `VoronoiCircle` and `BeachLineNodeKey`, which name what the predicates read of `voronoi_structures.hpp`'s types |
| `geometry_type` | voronoi-2 | `voronoi_geometry_type.hpp` 1-48 | `GeometryCategory`, `SourceCategory` (the C++'s enum values as constants, since sites OR them into their flags) and `belongs` |
| `structures` | voronoi-2 | `voronoi_structures.hpp` 1-450 | `point_2d` (`Point2d`), `site_event` (`SiteEvent`, whose `source_category` setter ORs into the flags and whose constructors leave the initial index 0), `circle_event` (`CircleEvent`, zero and active by default where the C++ leaves the coordinates unset), `ordered_queue` (`OrderedQueue`: the heap through `orca_stl::push_heap_by` and `pop_heap_by`, the `std::list` as slots that a pop frees for the next push, a slot standing for the list iterator), `beach_line_node_key` and `beach_line_node_data` (the circle event as a queue slot, the edge as the output's handle); they implement the traits of `predicates` |
| `formation` | voronoi-2 | `voronoi_predicates.hpp` 525-1533 | `mp_circle_formation_functor` (`MpCircleFormationFunctor`, its `int64` and `int32` products as the C++'s, which wrap where MSVC's do), `lazy_circle_formation_functor` (`LazyCircleFormationFunctor`) and `circle_formation_predicate` (`CircleFormationPredicate`, generic over the existence predicate and the formation functor, the C++'s `CEP` and `CFF`, through the traits `CircleExistence` and `CircleFormationFunctor`) |
| `tree` | voronoi-2 | MSVC 14.42.34433's `<xtree>` | `std::map` as MSVC builds it (`StdMap`): the red-black tree with its head node, `_Find_hint`, `_Find_lower_bound`, `_Insert_node`, `_Extract` and the iterators, in an arena whose freed nodes the next insert takes. The beach line's comparison is a strict order only on the keys the sweep compares, so the hint's checks and the tree's shape decide which comparisons run; this is why it is MSVC's map and not a `BTreeMap`. It belongs in `orca-stl`, which has no `std::map` yet |
| `builder` | voronoi-2 | `voronoi_builder.hpp` 1-521 | `voronoi_builder<int32>` (`VoronoiBuilder`, generic over the output's edge handle) and the output it fills (`VoronoiOutput`, the members `voronoi_diagram` gives the builder); the site sort and `unique` through `orca_stl::sort_by` and `unique_vec_by`, the end points in an `orca_stl::PriorityQueue` |
| `diagram` | voronoi-3 | `voronoi_diagram.hpp` 1-620 | `voronoi_cell`, `voronoi_vertex` and `voronoi_edge` (`VoronoiCell`, `VoronoiVertex`, `VoronoiEdge`), linked by their indexes in the diagram's containers where the C++ has pointers, the `mutable` colours as `Cell`s above the 5 low bits each element keeps (a cell's source category, an edge's linear and primary bits); `voronoi_diagram<double>` (`BoostVoronoiDiagram`) as the builder's `VoronoiOutput` with `usize` edge handles, and `_build` (the degenerate edges within 128 ulps removed, the incident edges set, the unused vertices removed, the infinite edges' loops closed) |
| `voronoi` | voronoi-3 | `voronoi.hpp` 1-157 | `insert` and `construct_voronoi` of points, of segments, or of points and then segments, over Boost's point and segment concepts (`PointConcept`, `SegmentConcept`, implemented for `Slic3r::Point` and `Slic3r::Line`); each coordinate narrowed to `int32` with `as i32`, the C++'s implicit conversion |
| `geometry_voronoi` | voronoi-3 | `Geometry/Voronoi.hpp` 1-212 | `Slic3r::Geometry::VoronoiDiagram` (`VoronoiDiagram`): its `IssueType` and `State`, the accessors that read the repair's copies once modified and the builder's diagram otherwise, the point and mixed overloads of `construct_voronoi` (no repair: the state and the issue `UNKNOWN`), and `Segment` with its segment traits |
| `polygons_point_index` | voronoi-3 | `Arachne/utils/PolygonsPointIndex.hpp` 1-178, `PolygonsSegmentIndex.hpp` 1-50 | `PathsPointIndex` over any `PathPoints` (`PolygonsPointIndex` for `Polygons`), its `unsigned int` stepping, `==` by the paths' address and `<` by the vertex; the two locators; `std::hash<PolygonsPointIndex>` through `point_hash` (`Slic3r::PointHash`, translated here for want of one in `orca-geom`); `PolygonsSegmentIndex`, which dereferences to its point index |
| `voronoi_utils` | voronoi-3 | `Geometry/VoronoiUtils.hpp` 1-167, `VoronoiUtils.cpp` 1-348 | `VoronoiUtils`: `to_point` (`llround` through `orca-crt`), `is_finite`, `make_rotated_vertex` (`cos` and `sin` through `orca-crt`), `is_in_range` against the limits as `double`s; `get_source_segment`, `get_source_point` and `get_source_point_index`, whose exceptions are `VoronoiError`'s variants with the C++'s texts; `discretize_parabola` in the C++'s wrapping `int64` arithmetic and its 32-bit `lround`; `compute_segment_cell_range` and `compute_point_cell_range` over the diagram's containers (`VoronoiContainers`); `norm_i64` (Eigen's `norm()` of an `int64` vector, which belongs in `orca-eigen`) |
| `repair` | voronoi-4 | `Geometry/Voronoi.cpp` 1-364 | The bodies of `VoronoiDiagram`'s methods (its members are `pub(crate)`): `construct_voronoi_segments` (`construct_voronoi` of segments, the repair on by default in the C++) with the detection, its warnings and errors through `orca_mesh::log` and the repair by rotations of π/6, π/5, π/7 and π/11; `clear`; `copy_to_local`; `detect_known_issues` and the edge and cell detections; the two repairs, the one by one rotation public as the probe calls it; the segment ends kept in vertex colours (`(index + 1) << 1 \| end`) so that they come back exact. The lookups' exceptions are `VoronoiError`s, returned |
| `voronoi_utils_cgal` | voronoi-4 | `Geometry/VoronoiUtilsCgal.hpp` 1-29, `.cpp` 1-334 | `VoronoiUtilsCgal`: `is_voronoi_diagram_planar_angle` (two linear edges by CGAL's `orientation` of `Simple_cartesian<double>`, which is plain `double` arithmetic; a parabolic edge by its tangent, CGAL's filtered predicates, whose answer is the exact sign, computed by `exact`) and `is_voronoi_diagram_planar_intersection` (a pairwise exact test that reports what CGAL's sweep reports: a crossing, or an end of one edge inside another; not a shared end). No CGAL code is copied |
| `exact` | voronoi-4 | none | The exact sign of a polynomial in `double`s: an interval filter widened by one ulp per operation, then exact dyadic numbers (`Interval`, `Dyadic`, `Ring`, `exact_sign`). Written for this crate from the definition of the signs; it belongs in `orca-exact` |
| `voronoi_offset` | voronoi-4 | `Geometry/VoronoiOffset.hpp` 22-103, `VoronoiOffset.cpp` 208-252 and 646-971 | The colour categories (`VertexCategory`, `EdgeCategory`, `CellCategory`, newtypes over the colour's low byte), `contour_point`, `vertex_point`, `detail::vertex_equal_to_point` (128 ulps), `detail::on_site`, `reset_inside_outside_annotations` and `annotate_inside_outside`, which MedialAxis calls. No piece carries VoronoiOffset; these parts belong in this crate |
| `medial_axis` | voronoi-4 | `Geometry/MedialAxis.hpp` 1-53, `MedialAxis.cpp` 1-707, `ExPolygon.cpp` 261-378 | `MedialAxis` (`build`, `build_polylines`, `process_edge_neighbors`, `validate_edge`) and `ExPolygon::medial_axis` of both outputs as the trait `ExPolygonMedialAxis` (`ExPolygon` is `orca-geom`'s); the largest width through `fmaxf` (written here from the C standard; it belongs in `orca-crt`), the widths' maximum through `orca_stl::max_element_f64`, the closing through `orca_geom::clipper_utils::closing_ex_expolygons` |

voronoi-4 leaves out: the `ColoredLines` instantiations of `Voronoi.cpp` and `VoronoiUtilsCgal.cpp`
(multi-material segmentation, outside domain A: `ColoredLine` is not translated), CGAL's
`impl::Epick` and `is_equal` (used in asserts only), MedialAxis's `SLIC3R_DEBUG` dump and its
templates that nothing instantiates (`retrieve_cell_point`, `measure_edge_thickness`,
`Lines2VDSegments`), and the rest of VoronoiOffset (offsets, skeleton edges, signed distances), which
no domain-A caller of this crate reaches. The asserts of the debug build are left out. Nothing of
voronoi-4 stops with NotPorted. A construction after a failed repair detects on the stale copies of
the last rotation with the new segments, as the C++ does; a site index past the segments there
panics where the C++ reads outside the vector, and a lookup that throws is returned.

Names: the C++ overloads that pick by their argument's concept or type are functions of their own
names: `construct_voronoi_points`, `construct_voronoi_segments` and
`construct_voronoi_points_segments`; `VoronoiUtils::to_point_ptr` (the pointer form),
`is_in_range_vertex` and `is_in_range_edge`. A C++ functor's `operator()` is `call`.

Names: each overload of a C++ `operator()` is a method of its own
(`EventComparisonPredicate::site_site`, `site_circle`, `circle_site`, `circle_circle`; `call`
where a class has one). `extended_int`'s in-place `add`, `dif` and `mul` are `set_add`, `set_dif`
and `set_mul`, and its `neg()` is `neg_in_place`; `robust_dif`'s `neg()` is `neg_sum`, since Rust's
operator traits take the other names. The builder's second `_insert_new_edge` is
`insert_new_edge_at_circle`. The struct `structures::BeachLineNodeKey` is not re-exported at the
crate root, where the trait `predicates::BeachLineNodeKey` has the name.

Left out, each named in its module's header: `extended_int`'s converting constructor and assignment
from another `extended_int<M>` (nothing instantiates them), and `robust_dif`'s one-value
constructor (it cannot be instantiated with `robust_fpt`); the builder's coordinate traits and
predicates as template parameters (`CTT` and `VP`: OrcaSlicer uses only the defaults for `int32`),
and the copy constructor and assignment of the builder and of `ordered_queue`, which the C++
disallows. The constructors that leave members unset set them to zero. Nothing of voronoi-1 or
voronoi-2 stops with NotPorted: domain A reaches all of it through the builder.

voronoi-3 leaves out: the diagram's copy constructor and assignment (disallowed in the C++), the
`enable_if` dispatch of `voronoi.hpp`'s overloads, the type aliases and the iterator types of
`Geometry/Voronoi.hpp` (slices here), and the `ColoredLines` instantiations of `VoronoiUtils`
(`MultiMaterialSegmentation.cpp`'s, the family `painted-facets`, outside domain A: `ColoredLine`
is not translated). The bodies of `Geometry/Voronoi.cpp` are voronoi-4's. Nothing of voronoi-3
stops with NotPorted. The page lists two of its edges as places a port may stop: a coordinate
outside 32 bits, which `insert` narrows as the C++ does (`as i32`, the same low 32 bits), and
`to_point` of a vertex outside the 64-bit range, which `llround` answers as the UCRT's does
(through `orca-crt`); the repair's cell pass reaches the second on the non-finite vertices of a
degenerate diagram, so a stop there would stop voronoi-4. Where the C++ reads past a container
unchecked (`get_source_point` past the segments, a point index past its path) or follows a null
pointer, the translation panics, the C++'s behaviour being undefined.

## How it is proven

`tests/probes.rs` replays the records of `voronoi.sop1` (`parity/SOP1.md`, "Probe files", and its
page `parity/sop1/voronoi.md`), which `bun run engine parity probe --only voronoi` writes from the
C++ engine's own build (`parity/oracle/probe/voronoi.cpp` and `parity/oracle/probe/voronoi/`),
piece by piece, each record compared whole, bit for bit.

voronoi-1 replays its thirteen kinds: VRUL (`ulp_comparison`), VREF (`extended_exponent_fpt`),
VRXC and VRXI (`extended_int`'s constructors and operations), VRRF and VRRD (`robust_fpt` and
`robust_dif`), VRSQ (`robust_sqrt_expr`), VRCP (`robust_cross_product` and the orientation tests),
VRPC and VREC (the point and event comparisons), and VRDP, VRNC and VRCE (the distance, node
comparison and circle existence predicates on the calls the builder made in the probe's runs).
Every record of both sets passed on 2026-11-13: 17,340 of the full set and 1,793 of the committed
set.

voronoi-2 replays six more: VRMP and VRLZ (the exact and the lazy circle formation functors on the
triples the builder's runs met, the exact one with all three recompute flags and with one other
set of them), VRCF (the circle formation predicate: whether its functor ran, its answer and the
circle), VRQU (the ordered circle event queue on scripts of pushes, deactivations and pops, many of
them ties), VRSS (the builder's sorted sites, after `std::sort` and `std::unique`) and VRBT (every
call the builder makes on its output for each of the probe's site sets, degenerate ones included:
the sites it names, the circle of each vertex by its bits and the edges' indexes, which pins the
whole sweep). Since voronoi-2 the predicates of voronoi-1 are replayed on the crate's own
`SiteEvent`, `CircleEvent` and `structures::BeachLineNodeKey`, a site built from a record as the
C++ builds it (`inverse()` for an inverse site). Every record of both sets passed on 2026-11-13:
9,483 of the full set (VRMP 4,118, VRLZ 2,059, VRCF 2,704, VRQU 42, VRSS 280, VRBT 280) in the
release profile and 283 of the committed set in the debug profile.

voronoi-3 replays six more: VRVD (Boost's `construct_voronoi` of points, segments or both, the
whole diagram after `_build`: every cell, vertex and edge with its links and colour, the vertices
by their bits and a NaN matching any NaN), VRVG (`Slic3r::Geometry::VoronoiDiagram`'s point and
mixed overloads: the issue, the state, the counts and the diagram through the accessors), VRUT
(the vertex helpers on hand-picked and seeded vertices, the limits of `int32` and `int64`, infinite
and NaN coordinates included), VRPI (`PolygonsPointIndex` and `PolygonsSegmentIndex` on every point
of Arachne's polygons: the steps, the locators, the hash and the comparisons), VRCR (the source
lookups and both cell ranges on every cell of raw diagrams of `Lines`, of `PolygonsSegmentIndex`es
and of points and lines, and the calls that throw, with the exceptions' kinds and texts) and VRDZ
(`discretize_parabola` on the parabolic edges Arachne discretizes and on seeded inputs, with the
warnings it logs). VRCR's sources 0 and 1 come from `construct_voronoi(..., false)` of segments,
whose body is voronoi-4's; with the repair off it is Boost's `construct_voronoi` and the state
`UNKNOWN`, which is what the mixed overload with no points does, so the replay builds them through
it. Every record of both sets passed on 2026-11-14: 2,855 of the full set (VRVD 280, VRVG 124,
VRUT 411, VRPI 113, VRCR 229, VRDZ 1,698) in the release profile and 360 of the committed set in
the debug profile, with every record of voronoi-1 and voronoi-2 still passing.

voronoi-4 replays the last four: VRGD (`construct_voronoi` of segments with the repair on and off,
on the shapes' lines, Arachne's `PolygonsSegmentIndex`es, `test_voronoi.cpp`'s line sets and the
repair search's draws: the issue, the state and the counts, the rotations the repair ran, each
rerun on a fresh object, the diagram a repair left, the log, and MedialAxis's second construction
on the closing's lines after a failed repair, which detects on the stale diagram and may throw),
VRCG (both planarity checks on raw diagrams of lines, of `PolygonsSegmentIndex`es and of points),
VRAN (`annotate_inside_outside` after a construction: the colour of every cell, vertex and edge)
and VRMA (`MedialAxis::build` and `ExPolygon::medial_axis` of both outputs on the regions the walls
and the fill give it: the thick polylines bit for bit, the plain ones' points, the errors and the
log). Every record of both sets passed on 2026-11-14: 1,553 of the full set (VRGD 929, VRCG 336,
VRAN 115, VRMA 173) in the release profile and 249 of the committed set (VRGD 63, VRCG 88, VRAN 25,
VRMA 73) in the debug profile, with every record of voronoi-1 to voronoi-3 still passing. The file
holds no other kind, and a kind that no piece names fails the gate.

```
bun run rs:test -p orca-voronoi
SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes bun run rs:test:release -p orca-voronoi --test probes
```

## Dependencies

Every library crate of phase O2 and every crate of phase O3 before it in the build order:
`orca-arrange`, `orca-config`, `orca-crt`, `orca-eigen`, `orca-exact`, `orca-fmt`, `orca-geom`,
`orca-json`, `orca-mesh`, `orca-model`, `orca-placeholder`, `orca-qhull`, `orca-stl`, `orca-tbb`,
`orca-xml`, `orca-zip`, `orca-index`, `orca-paths`. `orca-probe` for its tests. Nothing from
crates.io. voronoi-1 uses `orca-crt` (`frexp`, `ldexp`, `sqrt`) and `orca-stl` (`max`); voronoi-2
uses `orca-stl` (`sort_by`, `unique_vec_by`, `push_heap_by`, `pop_heap_by`, `PriorityQueue`);
voronoi-3 uses `orca-crt` (`llround`, `lround`, `cos`, `sin`, `atan`, `sqrt` and the float-to-int
casts), `orca-geom` (`Point`, `Line`, `Polygon`, `is_point_inside_polygon_corner`), `orca-eigen`
(`Vec2d`) and `orca-mesh` (`log`), and its tests `orca-geom` and `orca-mesh` too. voronoi-4 uses
`orca-crt` (`fabs`; `cos` and `sin` through `Point::rotated` and `make_rotated_vertex`), `orca-stl`
(`max_element_f64`), `orca-geom` (`ExPolygon`, `Polygon::intersection`, `Line`, `Polyline`,
`ThickPolyline`, `closing_ex_expolygons`, `to_lines_ex`), `orca-eigen` (`Vec2d`'s `norm` and
`normalized`) and `orca-mesh` (`log`).

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.
