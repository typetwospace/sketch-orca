# orca-arachne

Phase O3 of the Rust translation: libslic3r's Arachne wall generator: the beading strategies,
ExtrusionLine, the skeletal trapezoidation graph and SkeletalTrapezoidation, the grids, the polyline
stitcher and WallToolPaths.

Set up empty by orca-o3-skeleton on 2026-11-13: a manifest that declares every dependency the crate
will need, so that no unit that fills it changes `Cargo.toml` or `Cargo.lock`, and a `src/lib.rs`
with the header and the crate's doc comment. Each piece below adds its modules and its tests and
keeps this README true.

## The pieces that fill it

| Piece | What it adds |
| --- | --- |
| `orca-o3-arachne-1` | The beading strategies and ExtrusionLine (in, 2026-11-13) |
| `orca-o3-arachne-2` | The skeletal trapezoidation graph, the grids and the stitcher (in, 2026-11-14) |
| `orca-o3-arachne-3` | SkeletalTrapezoidation, first half (in, 2026-11-14) |
| `orca-o3-arachne-4` | SkeletalTrapezoidation, second half, and WallToolPaths |

The C++ they translate (under `src/libslic3r/` of OrcaSlicer v2.4.2 unless named):
Arachne/BeadingStrategy/*, Arachne/utils/ (ExtrusionJunction, ExtrusionLine, linearAlg2D, HalfEdge,
HalfEdgeGraph, HalfEdgeNode, SquareGrid, SparseGrid, SparseLineGrid, SparsePointGrid,
PolylineStitcher), Arachne/SkeletalTrapezoidationGraph, SkeletalTrapezoidationEdge,
SkeletalTrapezoidationJoint, SkeletalTrapezoidation and WallToolPaths. Arachne comes from
CuraEngine (copyright Ultimaker B.V.), with the PrusaSlicer and OrcaSlicer authors' changes, under
AGPL-3.0.

## Modules

What arachne-1 added (2026-11-13). Each module carries its C++ file whole, unless it says otherwise,
and its header says how.

- `beading`: `BeadingStrategy.hpp` and `.cpp`. The C++ abstract class is the trait
  `BeadingStrategy`: the base class's virtual methods are provided methods that call the others
  through `self`, so an override changes what the base bodies compute, as C++'s virtual calls do
  (`OuterWallInsetBeadingStrategy`'s anchor runs the base body through its own thicknesses, not its
  parent's anchor). `BeadingStrategyBase` holds the base fields, which a meta-strategy copies from
  its parent, name included. `Beading`, `BeadingStrategyPtr` (`Box<dyn BeadingStrategy>`) and
  `pi_div` (`pi_div(3)` deduces `int`, so the base constructor's default angle is 1).
  - `beading::distributed`, `limited`, `outer_wall_inset`, `redistribute`, `widening`: the five
    strategies, one file each, with the C++'s `float` and `double` operations in its order and its
    truncations through `orca_crt::f64_to_i64` and `f32_to_i64`.
  - `beading::factory`: `BeadingStrategyFactory::makeStrategy` (`make_strategy`, its thirteen
    arguments in order, and `StrategyArgs` with the header's defaults), logging its trace records
    before each strategy it stacks.
- `extrusion_junction`: `ExtrusionJunction.hpp`: the junction, `make_point`, `LineJunctions`.
- `extrusion_line`: `ExtrusionLine.hpp` lines 1-286 and 293-294 and `ExtrusionLine.cpp` lines 1-288:
  the line (`getLength` with Eigen's integer `norm`, `int64_t(sqrt(double(squaredNorm)))`;
  `simplify`; `calculateExtrusionAreaDeviationError`; `is_contour`; `area`; `toPolygon`),
  `to_thick_polyline` for these lines and for ClipperLib_Z's paths (`ThickPathPoint`),
  `to_polygon`, `to_points` and `VariableWidthLines`. `extrusion_paths_append` (header lines
  287-292, source lines 289-303) is the piece walls-1's in orca-perimeter; the header's `#if 0`
  block is not compiled and not carried.
- `linear_alg_2d`: `linearAlg2D.hpp`: `point_is_left_of_line` and `get_angle_left`.
- `half_edge`: `HalfEdge.hpp`, `HalfEdgeGraph.hpp` and `HalfEdgeNode.hpp`. The graph's two
  `std::list`s are `StableList`s: each element in a slot of its own that is never reused, linked
  in the list's order, so a handle (`EdgeId`, `NodeId`; `None` for `nullptr`) stands for a C++
  pointer: it stays valid as the list changes, compares as an address does, and is known once its
  element is erased (`get` gives `None`, `positions` gives -2, as the probe writes a pointer).
- `stop`: the stops (below), with orca-index's `NotPorted` payload.

What arachne-2 added (2026-11-14), each module carrying its C++ file whole:

- `skeletal_trapezoidation_edge`: `SkeletalTrapezoidationEdge.hpp`: the edge's type, its
  centrality (unknown at first; an unknown one reads as not central, as in the release build) and
  its three weak pointers to the transition middles, the transition ends and the extrusion
  junctions that SkeletalTrapezoidation owns while it runs. A `std::weak_ptr` is a `Weak` of an
  `Rc<RefCell<_>>` (`Shared`), so a list dies with its storage as in the C++; a `std::list` of
  transitions is a `StableList` (`TransitionMiddles`, `TransitionEnds`), whose handles (`ListIt`)
  stay valid as the list's iterators do.
- `skeletal_trapezoidation_joint`: `SkeletalTrapezoidationJoint.hpp`: the node's distance to the
  outline, bead count, transition ratio (`f32`) and weak pointer to its `BeadingPropagation`.
- `skeletal_trapezoidation_graph`: `SkeletalTrapezoidationGraph.hpp` and `.cpp`: the graph is
  `HalfEdgeGraph` of those data; the methods of `STHalfEdge` and `STHalfEdgeNode` follow pointers,
  so they are the graph's methods and take a handle (`can_go_up`, `is_upward`, `dist_to_go_up`,
  `get_next_unconnected`, `is_multi_intersection`, `is_central_node`, `is_local_maximum`).
  `collapse_small_edges` walks the edge list in order as the C++ does (the C++'s two ankerl maps
  from an element's address to its iterator are not needed: a handle is the iterator) and writes
  the C++'s `std::cerr` lines past 50 edges round a node; `make_rib` adds at the front of the
  lists, `insert_rib` and `insert_node` at the back. `shorter_then` (Point.hpp, which belongs in
  orca-geom) and the `operator<<` of a `Point` are translated here.
- `square_grid`: `SquareGrid.hpp` and `.cpp`: the coordinates (truncating division, so the cells at
  0 are twice as large, as the C++ says it means) and the two walks, `process_line_cells` (a fat
  line, each row's last cell again as the next row's first) and `process_nearby` (row by row).
- `sparse_grid`, `sparse_point_grid`, `sparse_line_grid`: `SparseGrid.hpp`, `SparsePointGrid.hpp`
  and `SparseLineGrid.hpp`. The C++ derives each from the one before; here each holds it and
  dereferences to it. A locator is a trait (`PointLocator`, `LineLocator`) that orca-voronoi's
  `PathsPointIndexLocator` and `PolygonsPointIndexSegmentLocator` and any closure implement.
- `unordered_multimap`: MSVC 14.42's `std::unordered_multimap` (from `xhash`, Apache-2.0 WITH
  LLVM-exception), the parts the grids call: `emplace` (after the last element of its key, or at the
  front of its bucket's run), the growth and the rehash in list order, `equal_range` and the
  iteration order. It belongs in orca-stl, whose README says it covers no multimap yet.
- `polyline_stitcher`: `PolylineStitcher.hpp` and `.cpp`: `stitch` of the `VariableWidthLines`
  instance and its three specialisations (`can_reverse`, `can_connect`, `is_odd`);
  `ExtrusionLine` implements orca-voronoi's `PathPoints` here, so orca-voronoi's `PathsPointIndex`
  indexes the lines. The `Polygons` instance stops (below).

What arachne-3 added (2026-11-14):

- `skeletal_trapezoidation`: `SkeletalTrapezoidation.hpp` whole and `SkeletalTrapezoidation.cpp`
  lines 1-1268, with `isEndOfCentral` (lines 1336-1355), which the steps here call. The
  constructor (`SkeletalTrapezoidation::new`) is `constructFromPolygons`: orca-voronoi's
  `construct_voronoi_segments` with the repair on (the detection of known issues, the planarity
  check among them, and the rotation), each cell's range transferred into the graph
  (`transferEdge`, `discretize`, `makeRib`), then `separatePointyQuadEndNodes`,
  `collapseSmallEdges(5)` and the incident edges. The C++'s two ankerl maps from a Voronoi edge's
  and vertex's address are `BTreeMap`s keyed by the element's index in the diagram (only looked
  up, never walked). The lookups' exceptions are returned (`VoronoiError`). The transitioning
  steps of `generateToolpaths` are public, as the probe's derived class exposes them:
  `update_is_central`, `filter_central`, `update_bead_count`, `filter_noncentral_regions`,
  `generate_transition_mids`, `filter_transition_mids` (with `dissolve_nearby_transitions`,
  `dissolve_bead_count_region`, `filter_end_of_central_transition`) and
  `generate_all_transition_ends` (with `generate_transition_ends`, `generate_transition_end`,
  `is_going_down`). An overload gets a name of its own (`filter_central_from`,
  `filter_noncentral_regions_from`). `generate_toolpaths` and `generate_transitioning_ribs` are
  written whole; the three steps they call that are arachne-4's (`apply_transitions`,
  `generate_extra_ribs`, `generate_segments`) stop until arachne-4 writes their bodies. `normal`
  (the file-local helper of `applyTransitions`) is public for arachne-4.

## Stops

The C++ release build does not run Arachne's asserts and has no answer on some inputs (undefined
behaviour). Domain A never reaches them, no probe record holds one, and the crate panics with
orca-index's `NotPorted` payload, its `function` the C++ function and its `condition` the edge
(`parity/sop1/arachne.md`, "The pieces and their edges", for the first two):

- `limited-below-2`: `LimitedBeadingStrategy::compute` with a bead limit below 2 indexes -1.
- `line-shape`: `ExtrusionLine::area` of an open line, `to_thick_polyline` of fewer than two
  points, `to_polygon` of fewer than three junctions.
- `limited-short-parent` (this crate's name): `LimitedBeadingStrategy::compute` over the limit
  when the parent gave fewer than half the limit's beads reads past the end; none of the five
  strategies does.
- `area-error-no-length` (this crate's name): `calculateExtrusionAreaDeviationError` of three
  junctions at one point whose widths differ by more than 1 divides by zero; `simplify` calls it
  only after a segment of 5 microns or more.
- `collapse-no-twin`: `collapseSmallEdges` meeting a quad middle edge to collapse that has no twin
  logs a warning and `continue`s without moving on, for ever; the crate logs the warning once and
  stops.
- `graph-null-walk`: a null pointer the graph's code follows: a node's null incident edge in
  `isCentral` and `isLocalMaximum`, a null next edge between equidistant nodes in `canGoUp` and
  `distToGoUp`, and any other null edge, twin or node the graph's functions read through (the C++
  crashes on each). The early returns the C++ writes beside its asserts are kept.
- `stitch-polygons`: `PolylineStitcher<Polygons, Polygon, Point>`, which domain A never uses
  (`stitch_polygons`).
- `stitch-empty-line`: `stitch` on a line with no junction reads its first and last junction
  outside the vector.
- `transfer-no-twin`: `transferEdge`, walking the pieces of a twin made before, meets a null twin:
  it logs a warning and `continue`s, and the loop's step follows the null pointer; the crate logs
  the warning and stops.
- `filter-outer-central`: `filterOuterCentral`, which only `generateToolpaths` with
  `filter_outermost_central_edges` calls; WallToolPaths never asks for it.
- `transitions-empty` (this crate's name): `filterTransitionMids` or `isGoingDown` taking the front
  or back of an empty transition list, popping one, or erasing a transition a previous erase took
  out.
- `skeletal-trapezoidation-second-half` (this crate's name, temporary): `apply_transitions`,
  `generate_extra_ribs` and `generate_segments`, the piece arachne-4's, which removes the stop when
  it writes them.

## How it is proven

`tests/probes.rs` replays the records of `arachne.sop1` (`parity/SOP1.md`, "Probe files", and
its page `parity/sop1/arachne.md`), which `bun run engine parity probe --only arachne` writes from
the C++ engine's own build (`parity/oracle/probe/arachne.cpp` and `arachne/`). Each record is rebuilt
from the crate and compared field by field, every `float` and `double` by its bits:

- arachne-1's kinds: ANBS (the beading strategies and the factory's stack, with their logs), ANLA
  (`LinearAlg2D`), ANXL (`ExtrusionLine`'s operations) and ANAE (the area error). Every record of
  the committed set (in the debug profile) and of the full set (in the release profile) matches.
- arachne-2's kinds: ANSQ (`SquareGrid`'s coordinates and walks, each walk's cells in order and
  where a stop ends it), ANSP (a `SparsePointGrid`'s buckets, cells in iteration order and
  `getNearby` in order), ANSL (`createLocToLineGrid`'s `SparseLineGrid` of polygon segments, its
  iteration order and `getNearby`; the test builds the grid as WallToolPaths.cpp lines 247-260 do,
  which arachne-4 translates), ANPS (`PolylineStitcher::stitch` of `VariableWidthLines`) and ANGR
  (the graph: `collapseSmallEdges`, `insertNode` and `insertRib` each on a copy of the recorded
  graph, which is compared whole after, and the edge and node queries on the graph itself; the
  probe's copy writes an erased element's pointer as null, its picks were made on the original).
- arachne-3's kinds: ANSC (a trapezoidation built from the outline, strategy and parameters the
  record holds, with the log of the strategy's construction and the constructor's) and ANST stages
  0 to 7 (the graph after the constructor, then after each step from `updateIsCentral` to
  `generateAllTransitionEnds`, each step run on the graph built from the record of the stage
  before, with its log). Every record of the committed set (10 trapezoidations, debug profile) and
  of the full set (100, release profile) matches. A mutation check on a scratch copy killed 10 of
  13 mutants; the survivors are `separatePointyQuadEndNodes` adding its copy at the front (no
  record duplicates a node) and two `float` products done in `double` (`updateIsCentral`'s
  comparison and the lower transition half length), which no record tells apart.
- The kinds of arachne-4 (ANST stages 8 to 10, ANWS, ANIP, ANWH, ANWT, ANMP) are listed as its own;
  it adds its replay. A record of no piece fails the test.

The probe's log capture sees only warning and above: libslic3r sets Boost.Log's core filter when it
is loaded (`utils.cpp`, `RunOnInit`, `set_logging_level(2)`), so the factory's trace records are
not in the records though the crate logs them; the test's sink keeps the same records.

```
bun run rs:test -p orca-arachne
SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes bun run rs:test:release -p orca-arachne --test probes
```

## Dependencies

Every library crate of phase O2 and every crate of phase O3 before it in the build order:
`orca-arrange`, `orca-config`, `orca-crt`, `orca-eigen`, `orca-exact`, `orca-fmt`, `orca-geom`,
`orca-json`, `orca-mesh`, `orca-model`, `orca-placeholder`, `orca-qhull`, `orca-stl`, `orca-tbb`,
`orca-xml`, `orca-zip`, `orca-index`, `orca-paths`, `orca-voronoi`, `orca-slice`, `orca-layer`.
`orca-probe` for its tests. Nothing from crates.io. arachne-1 uses `orca-geom` (points, lines,
polygons, `ThickPolyline`, ClipperLib_Z's points), `orca-crt` (`sqrt`, `atan2` and the casts),
`orca-stl` (`std::min` and `std::max`), `orca-mesh` (the log) and `orca-index` (`NotPorted`).
arachne-2 adds `orca-voronoi` (`PointHash`, `PathsPointIndex`, `PolygonsPointIndex` and their
locators), `orca-fmt` (a `Point`'s `operator<<`, the default stream's `double`) and `orca-crt`'s
`ceilf` (the multimap's `reserve`). arachne-3 adds orca-voronoi's `VoronoiDiagram` (the
construction and the repair), `VoronoiUtils` (the cell ranges, the source lookups,
`discretize_parabola`) and `PolygonsSegmentIndex`, and `orca-crt`'s `sin` and `tan`.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.
