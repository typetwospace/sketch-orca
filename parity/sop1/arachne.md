# arachne.sop1

The record kinds of the arachne probe (`parity/oracle/probe/arachne.cpp` and the sources of
`parity/oracle/probe/arachne/`, phase O3): the beading strategies, ExtrusionLine, the grids and the
stitcher, the skeletal trapezoidation and WallToolPaths, recorded by orca-o3-probe-arachne
(2026-11-13) from the oracle's libslic3r. The values are those of `parity/SOP1.md` ("The container",
"Values" and "Probe files"). Every kind starts with `AN`, and no other page defines it
(`scripts/lib/parity/probe.ts`, `probeKindProblems`, which `test/parity-probe.test.ts` runs).
`orca-arachne` replays the records: each section below names the piece whose port replays it
(arachne-1, arachne-2, arachne-3 or arachne-4), and the crate's probe test rebuilds every record of
its piece from the Rust and compares it whole.

A record holds its inputs, then what the engine computed from them. Inputs are shapes the probe
builds (seeded with the splitmix64 of `parity/oracle/probe/probe.hpp`, seeds `0xA7A1A000` to
`0xA7A1C500` and `0xA7AC4E01`) and slices of the OBJ meshes of OrcaSlicer's tests
(`<exe>/../src/tests/data`: `A.obj`, `V.obj`, `frog_legs.obj`, `extruder_idler.obj`, `ipadstand.obj`,
`cube_with_concave_hole.obj`, `two_hollow_squares.obj`, `bridge.obj`, `overhang.obj`,
`small_dorito.obj`, `sloping_hole.obj` and `pyramid.obj`, each cut by `slice_mesh_ex` at 0.37 and
0.61 of its height), never the corpus. The probe groups records into cases. Every case goes into
the full set (`<work>/parity/probes/arachne.sop1`). A case the probe marks goes into the committed
set (`parity/probes/arachne.sop1`) whole, while its family's share of the 512 KB budget holds it
(`parity/oracle/probe/arachne/common.cpp`, `budgets`), so a replay never meets half a case.

## arachne.sop1's values

`point` and `polygon` are those of `parity/SOP1.md` ("Values"). Besides them:

- points: `vec<point>`; polygons: `vec<polygon>`;
- a junction (`ExtrusionJunction`): `point` p, `i64` width, `u64` perimeter index;
- a line (`ExtrusionLine`): `u64` inset index, `bool` odd, `bool` closed, `vec<junction>`; lines
  (`VariableWidthLines`): `vec<line>`; toolpaths: `vec<lines>`, one per inset;
- a beading (`BeadingStrategy::Beading`): `i64` total thickness, `vec<i64>` bead widths, `vec<i64>`
  toolpath locations, `i64` left over;
- a log: a `u64` count of Boost.Log records the engine wrote since the last log of the run, each
  `u8` severity (trace 0, debug 1, info 2, warning 3, error 4, fatal 5) and `str` message, then
  `str` the text it wrote to `std::cerr` in that time. Every severity is captured, whatever the
  CLI's level; the port writes them through `orca_mesh::log` at the same severity;
- an optional value: `bool` present, then the value when present;
- a strategy: `u8` 1 for `BeadingStrategyFactory::makeStrategy`, then its thirteen arguments in
  order: `i64` outer width, `i64` inner width, `i64` transition length, `f32` transitioning angle,
  `bool` print thin walls, `i64` minimum bead width, `i64` minimum feature size, `f64` split middle
  threshold, `f64` add middle threshold, `i64` maximum bead count, `i64` outer wall offset, `i32`
  inward distributed centre wall count, `f64` minimum variable line ratio. Or `u8` 0 for a chain the
  probe builds itself, each strategy on the one before: a `u64` count, each `u8` kind and its
  constructor's arguments: 0 `DistributedBeadingStrategy` (`i64` optimal width, `i64` default
  transition length, `f64` transitioning angle, `f64` split middle threshold, `f64` add middle
  threshold, `i32` distribution radius), 1 `RedistributeBeadingStrategy` (`i64` outer width, `f64`
  minimum variable line ratio), 2 `WideningBeadingStrategy` (`i64` minimum input width, `i64`
  minimum output width), 3 `OuterWallInsetBeadingStrategy` (`i64` offset), 4
  `LimitedBeadingStrategy` (`i64` maximum bead count). The first of a chain is always kind 0;
- a graph (`SkeletalTrapezoidationGraph`): `u64` node count, `u64` edge count, then the nodes and
  the edges in their lists' order. A pointer is written as an `i32` index into its list: -1 for
  none, -2 for an object that is no longer in the list. A node: `point` p, `i64` distance to the
  boundary, `i64` bead count, `f32` transition ratio, `i32` incident edge, `bool` beading alive,
  and when alive its beading, `i64` distance to the bottom source, `i64` distance from the top
  source and `bool` upward propagated only. An edge: `u8` type (0 normal, 1 extra VD, 2 transition
  end), `u8` central (255 not yet set, 0 no, 1 yes), `i32` from, `i32` to, `i32` twin, `i32` next,
  `i32` prev; then `bool` transitions alive (the weak pointer, empty lists included) and when alive
  a `vec` of `i64` position, `i32` lower bead count, `i64` feature radius; `bool` transition ends
  alive and a `vec` of `i64` position, `i32` lower bead count, `bool` lower end; `bool` junctions
  alive and a `vec<junction>`;
- a wall setup: `str` name, `f64` nozzle, `f64` layer height, `i64` bead width 0, `i64` bead width
  x, `u64` inset count, `i64` outer wall inset, `i32` layer id, `bool` top or bottom layer;
- wall parameters (`WallToolPathsParams`): `f32` minimum bead width, `f32` minimum feature size,
  `f32` minimum length factor, `f32` transition length, `f32` transition angle, `f32` transition
  filter deviation, `i32` distribution count, `bool` top or bottom layer, `i64` maximum resolution,
  `i64` maximum deviation;
- a preparation step: `u8` op (0 `simplify`, 1 `fixSelfIntersections`, 2 `removeDegenerateVerts`,
  3 `removeColinearEdges`, 4 `removeSmallAreas`, the file-local functions of `WallToolPaths.cpp`),
  `i64` a (`simplify`'s smallest segment, `fixSelfIntersections`' epsilon), `i64` b (`simplify`'s
  allowed distance), `f64` f (`removeColinearEdges`' angle, `removeSmallAreas`' minimum area),
  `bool` remove holes (`removeSmallAreas`).

## The pieces and their edges

arachne-1 replays ANBS, ANLA, ANXL and ANAE. arachne-2 replays ANSQ, ANSP, ANSL, ANPS and ANGR.
arachne-3 replays ANSC and the ANST records of stages 0 to 7. arachne-4 replays the ANST records of
stages 8 to 10, ANWS, ANIP, ANWH, ANWT and ANMP. A stage's input is the graph of the stage before,
so a replay builds the graph from that record and runs the one step.

The probe calls only what domain A calls, the way it calls it. Where the engine has an edge that
domain A never reaches, or that is undefined in C++, the port stops with NotPorted naming it, and
no record holds it:

- `LimitedBeadingStrategy::compute` with a maximum bead count below 2 indexes -1 (`limited-below-2`).
- `SkeletalTrapezoidationGraph::collapseSmallEdges` meeting a quad middle edge with no twin loops
  for ever on its `continue` (`collapse-no-twin`); so does `transferEdge` meeting a null twin
  (`transfer-no-twin`).
- `generateToolpaths` with `filter_outermost_central_edges` (`filterOuterCentral`) is never called
  (`filter-outer-central`).
- `PolylineStitcher<Polygons, Polygon, Point>` is never used (`stitch-polygons`); `stitch` on a
  line with no junction reads before it (`stitch-empty-line`).
- `ExtrusionLine::area` of an open line, `to_thick_polyline` of fewer than 2 junctions and
  `to_polygon` of fewer than 3 assert (`line-shape`).
- `STHalfEdgeNode::isCentral` and `isLocalMaximum` dereference a null incident edge, and
  `STHalfEdge::canGoUp` and `distToGoUp` a null next edge between equidistant nodes
  (`graph-null-walk`); domain A calls them only where they are set.
- The four-argument `SkeletalTrapezoidation::interpolate` reads past the right beading unless it has
  at most one toolpath location fewer than the left one (`interpolate-short-right`).
- `make_paths_params` leaves the minimum feature size, minimum bead width, transition filter
  deviation or transition length unset when its option is 0, since the option's value is its truth
  test (`paths-params-zero`).

Two orders a port must take from orca-stl, not from its own containers: `std::unordered_multimap`'s
(SparseGrid's cells, so ANSP and ANSL record the iteration order) and `std::sort`'s on ties (the
upward edges `generateSegments` sorts, whose comparator leaves some pairs unordered). ankerl's sets
and maps iterate in insertion order.

The beading propagation (`propagateBeadingsUpward` and `propagateBeadingsDownward`), the junctions
and their connection run inside `generateSegments` on lists it owns, after a sort and a store it
does inline. They cannot be called on their own without copying that code, so ANST stage 10 holds
their result (the toolpaths), and ANIP holds the interpolation they use.

## Records

### ANBS

A beading strategy (arachne-1). `str` name; the strategy; a log (its construction); `str`
`toString()`; `f64` `getSplitMiddleThreshold()`; `f64` `getTransitioningAngle()`. Then a `u64`
count K and, for each lower bead count k from 0 to K - 1: `i64` `getOptimalThickness(k)`, `i64`
`getTransitionThickness(k)`, `i64` `getTransitioningLength(k)`, `f32` `getTransitionAnchorPos(k)`,
`vec<i64>` `getNonlinearThicknesses(k)`. K is the larger of the bead limit and 2, plus 2, for a
factory stack or a chain ending in a limited strategy, else 11. Then a `u64` count of thicknesses,
each `i64` thickness t, `i64` `getOptimalBeadCount(t)`, a `u64` count n and n times `i64` bead
count c and the beading `compute(t, c)`. The thicknesses are a grid from 0 to K widths in steps of
a fifth of a width (the factory's larger width, a chain's first width), each computed with its
optimal bead count and one more; then, in increasing order and without repeats, each transition
thickness, one below and one above it, and each optimal thickness, for k from 0 to K - 2, each
computed with one bead fewer (when the optimal count is above 0), its optimal count and one more.
Then a log.

### ANLA

linearAlg2D (arachne-1). `point` p, `point` a, `point` b, `point` c; `i64`
`pointIsLeftOfLine(p, a, b)`; `f32` `getAngleLeft(a, b, c)`.

### ANXL

An extrusion line's operations (arachne-1). The line; `i64` `getLength()`; `bool` `is_contour()`;
`bool` closed, and for a closed line `f64` `area()`. `bool` thick (two junctions or more), and
then `to_thick_polyline`: points, a `u64` count of `f64` widths, `bool` first endpoint, `bool`
last endpoint. `bool` polygon (a closed line of three junctions or more whose ends meet), and then
`to_polygon`'s points. `to_points`' points. Then a `u64` count, each `i64` smallest line segment
squared, `i64` allowed error distance squared, `i64` maximum extrusion area deviation, and the line
after `simplify` with them (each on a copy of the line).

### ANAE

`ExtrusionLine::calculateExtrusionAreaDeviationError` (arachne-1). Junctions a, b and c; `i64` the
error.

### ANSQ

SquareGrid (arachne-2). `i64` cell size, `u8` op, then by op: 0 `toGridCoord` and 1 `toLowerCoord`:
`i64` value, `i64` result. 4 `toGridPoint`: `point`, `point` result. 2 `processLineCells`: `point`
start, `point` end, `i64` stop, points (the cells handed to the callback, in order), `bool` result.
3 `processNearby`: `point` query, `i64` radius, `i64` stop, points, `bool` result. The callback
returns false once it has taken `stop` cells; a stop of -1 never stops it.

### ANSP

SparsePointGrid of an element the probe defines (a point and a `u64` id) (arachne-2). `i64` cell
size, `u64` element reserve, `f32` maximum load factor. A `u64` count of inserted elements, each
`point` and `u64` id, in insertion order. A `u64` count, each `point` grid cell and `u64` id, in
the grid's iteration order (`begin` to `end`). A `u64` count of queries, each `point` query, `i64`
radius, `vec<u64>` the ids of `getNearby`, `vec<u64>` the ids `processNearby` hands over to a
callback that stops after the second, `bool` its result.

### ANSL

SparseLineGrid of polygon segments as `WallToolPaths.cpp`'s `createLocToLineGrid` builds it
(arachne-2). `str` shape name; polygons; `i32` square size (2 mm as `fixSelfIntersections` builds
it, and 0.25 mm). A `u64` count, each `point` grid cell, `u32` polygon index, `u32` point index, in
the grid's iteration order. A `u64` count of queries, each `point`, `i64` radius, a `u64` count and
each `u32` polygon index, `u32` point index of `getNearby`.

### ANPS

`PolylineStitcher<VariableWidthLines, ExtrusionLine, ExtrusionJunction>::stitch` (arachne-2). The
input lines; `i64` maximum stitch distance; `i64` snap distance; the result lines; the result
polygons. The inputs are closed and open lines cut into pieces that share their cut junctions, some
ends moved, odd (and some even) pieces reversed and the pieces shuffled.

### ANGR

The graph's own operations, on copies of the graphs of an ANST run (arachne-2). `str` shape name;
`u8` op; the input graph. Then by op:

- 0 `collapseSmallEdges` on the graph after construction: a `u64` count, each `i64` snap distance
  and the graph after it.
- 1 `makeRib` on the first three edges with a previous edge, a next edge and a twin, each on its
  own copy, with the quad's first node and last node as the source points: a `u64` count, each
  `i32` edge, `point` start source, `point` end source, `i32` the edge `makeRib` leaves in its
  `prev_edge`, the graph.
- 2 `insertNode` on the first three central, not extra-VD, upward edges longer than 0.04 mm of the
  graph after `filterNoncentralRegions`, each on its own copy, at the edge's middle (`(from + to) /
  2`) with the smaller of its nodes' bead counts: a `u64` count, each `i32` edge, `point` middle,
  `i64` bead count, `i32` the edge it returns, the graph.
- 3 `isLocalMaximum()` of every node, on the graph `updateBeadCount` reads: a `u64` count of
  `bool`.
- 4 for each edge with a previous and a next edge of the graph `generateSegments` reads: a `u64`
  count, each `i32` edge, `bool` `isUpward()`, optional `i64` `distToGoUp()`, optional `i64` its
  twin's `distToGoUp()`.

### ANSC

A skeletal trapezoidation's inputs (arachne-3). `str` shape name; the wall setup; polygons (the
outline `WallToolPaths::generate` hands over: its preparation chain, as ANWH form 0 records it);
the strategy (`makeStrategy` with generate's arguments); `f64` transitioning angle (the strategy's
`getTransitioningAngle()`); `i64` discretization step size; `i64` transition filter distance;
`i64` allowed filter deviation; `i64` beading propagation transition distance; a log (the
strategy's and the constructor's). The ANST records of stages 0 to 10 follow it.

### ANST

One step of `SkeletalTrapezoidation::generateToolpaths`, run on the object of the ANSC before it
(stages 0 to 7 arachne-3, stages 8 to 10 arachne-4). `u8` stage, then for stages 0 to 9 the graph
after the step and a log. The stages: 0 the constructor (`constructFromPolygons` with
`separatePointyQuadEndNodes`, `collapseSmallEdges` and the incident edges); 1 `updateIsCentral`; 2
`filterCentral(central_filter_dist())`; 3 `updateBeadCount`; 4 `filterNoncentralRegions`; then
`generateTransitioningRibs`' steps with its two lists alive until it ends: 5
`generateTransitionMids`, 6 `filterTransitionMids`, 7 `generateAllTransitionEnds`, 8
`applyTransitions`; 9 `generateExtraRibs`. Stage 10 is `generateSegments`: toolpaths, a log, and
`bool` whether a direct `generateToolpaths` on a new object gave the same toolpaths (a check of the
probe's stepping; true in every record).

### ANWS

WallToolPaths' steps after the trapezoidation, on the toolpaths of an ANST stage 10 (arachne-4).
`str` shape name; the wall setup; the wall parameters; the toolpaths; the toolpaths after
`stitchToolPaths(toolpaths, bead width x)`; after `removeSmallLines`; after `simplifyToolPaths(
toolpaths, parameters)`; after `removeEmptyToolPaths`, and `bool` what it returned; a log. Then
`bool` a check of the probe: that `getToolPaths` gives these toolpaths without their zero-width
insets (which `separateOutInnerContour` takes out between `removeSmallLines` and
`simplifyToolPaths`; it works on WallToolPaths' private toolpaths, so ANWT holds it).

### ANIP

`SkeletalTrapezoidation::interpolate` (arachne-4). The left beading; `f64` ratio of left to
whole; the right beading; optional `i64` switching radius; the beading the four-argument form
returns when the radius is present, else the three-argument form's.

### ANWH

The outline preparation of `WallToolPaths::generate` (arachne-4). `u8` form. Form 0, the chain on a
shape: `str` name; polygons outline; `i64` smallest segment; `i64` allowed distance; `i64` bead
width 0; polygons after the triple offset (`offset(offset(offset(outline, -e), 2e), -e)` with e
the allowed distance / 2 - 1); a `u64` count of steps, each the step and the polygons after it (the
next step's input); polygons after `union_`. Form 1, one step on its own: the step, polygons in,
polygons out.

### ANWT

WallToolPaths whole, as PerimeterGenerator calls it (arachne-4). `str` shape name; the wall setup;
the wall parameters (`make_paths_params`, then the top or bottom layer flag); polygons outline; a
log (the constructor, `getToolPaths` and `getInnerContour`); the toolpaths of `getToolPaths`; polygons
`getInnerContour()`. Then twice, for the outer wall last and first: `bool` outer first, `u64` count
of lines (all lines of the toolpaths, inset by inset from the innermost when the outer wall goes
last, from the outermost when it goes first), `u64` count of pairs, each `i32` before and `i32`
after (indexes into those lines) of `getRegionOrder(lines, outer first)` in the set's order. Then a
log.

### ANMP

`make_paths_params` (arachne-4). Its inputs: `i32` layer id, `f64` `min_bead_width` (percent), `f64`
`initial_layer_min_bead_width` (percent), `f64` `min_feature_size` (percent), `f64`
`min_length_factor`, `f64` `wall_transition_filter_deviation` (percent), `f64`
`wall_transition_length` (percent), `f64` `wall_transition_angle`, `i32` `wall_distribution_count`,
`f64` `wall_maximum_resolution`, `f64` `wall_maximum_deviation`, `vec<f64>` nozzle diameters; the
other options at their defaults. Then the wall parameters it returns. No record sets an option whose
zero leaves a field unset (`paths-params-zero`).
