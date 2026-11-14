# orca-paths

Phase O3 of the Rust translation: libslic3r's ShortestPath: the chaining of extrusions, polylines
and islands into short travel orders.

Set up empty by orca-o3-skeleton on 2026-11-13: a manifest that declares every dependency the crate
will need, so that no unit that fills it changes `Cargo.toml` or `Cargo.lock`, and a `src/lib.rs`
with the header and the crate's doc comment. Each piece below adds its modules and its tests and
keeps this README true.

## The pieces that fill it

| Piece | What it adds |
| --- | --- |
| `orca-o3-paths-1` | ShortestPath's chaining (2026-11-13) |

The C++ they translate (under `src/libslic3r/` of OrcaSlicer v2.4.2 unless named): ShortestPath.hpp,
ShortestPath.cpp, ClipperUtils.cpp (lines 992-1017), Point.hpp (`ClosestPointInRadiusLookup`).

## The modules

paths-1 translates ShortestPath statement by statement, so every chaining picks the order the C++
picks, ties included: the KD tree and the mutable priority queue are orca-index's, sorts go through
orca-stl, and `fabs` and the float-to-integer cast through orca-crt.

| Module | What it carries |
| --- | --- |
| `segments` | ShortestPath.cpp 1-19 and 42-1025: the generic segment chaining every other chaining instantiates. `chain_segments_closest_point`; the multi-fragment greedy chaining with reversal constraints (`chain_segments_greedy_constrained_reversals_`: a KD tree over the end points, a priority queue of their nearest free neighbours, `EquivalentChains` for the joined fragments); its second version, which also flips whole fragments and stops after `16 * num_segments` rounds (`chain_segments_greedy_constrained_reversals2_`, `SegmentChain`, `Chains`); the four wrappers, `chain_segments_greedy` among them. |
| `points` | ShortestPath.hpp 1-20 and 29-47, ShortestPath.cpp 1096-1115 and 1998-2012: `chain_points`, `chain_expolygons` (each island by its box's center), `chain_path_items`, `chain_clipper_polynodes` (over orca-geom's `NodeId`s) and `reorder_by_shortest_traverse` (polylines, thick polylines and polygons, by their first points). |
| `polylines` | ShortestPath.cpp 1116-1996 as the release build compiles it, and hpp 29-31: `chain_polylines` (the second version's chaining, then, with no start point, two-exchanges with segment flipping: `FlipEdge`, `ConnectionCost`, `minimum_crossover_cost`, `do_crossover`, `reorder_by_two_exchanges_with_segment_flipping`), `chain_polylines_copy` and `FourOptCosts`. |
| `lines` | ShortestPath.cpp 2043-2099 and hpp 56-62: `chain_lines`, with its function-local static, and `chain_lines_against`, which takes that static's value as an argument. It also holds Point.hpp's `ClosestPointInRadiusLookup` (lines 506-636), which belongs in orca-geom beside `Point`; no piece carries it there. |
| `outside_in` | ClipperUtils.cpp 992-1017: `union_pt_chained_outside_in` and `traverse_pt_outside_in`, which walk orca-geom's `union_pt` tree from the outside in, each level in `chain_clipper_polynodes`' order, holes turned counter-clockwise. orca-geom leaves them out because they need ShortestPath. |
| `stop` | `NotPorted` (orca-index's payload) and the names of the edges below. |

Each module's header comment says what the C++ does that the translation keeps. The kept quirks
that change a result:

- With one segment the first version reverses it when its last point is nearer the start and it
  may reverse; the second reverses it when its first point is nearer, whether or not it may.
- The second version's loop may end by its limit before it has joined every segment; with a start
  point the chain it returns is then shorter, as the C++'s is.
- `chain_lines` compares against the square of the epsilon of the process's first call (a
  function-local static), and its lookup's equal ranges keep MSVC's `unordered_multimap` order (a
  new value after the last of its key; `erase` takes the first equal one).
- After the two-exchanges `chain_polylines` reverses a polyline whose edge ends where the polyline
  starts, so a closed polyline is always reversed.

## What stops with NotPorted, and why

OrcaSlicer's release build defines `NDEBUG`, so ShortestPath's asserts do not run. On these inputs
the C++ reads outside its data, which is undefined behaviour; domain A never reaches them, so the
crate stops with a `NotPorted` panic payload naming the C++ function and the edge:

- `empty-polyline`: the first or last point of an empty polyline or contour, in
  `chain_polylines`, `improve_ordering_by_two_exchanges_with_segment_flipping`,
  `reorder_by_shortest_traverse` and `traverse_pt_outside_in` (`parity/sop1/paths.md`, "Edges");
- `no-end-point`: a KD tree search of the chaining that finds no end point its filter keeps, so the
  C++ would take `&end_points[npos]`; the chaining always leaves one;
- `no-first-point`: the second version's loop ended by its limit with no start point given, so the
  walk would start from a null end point.

## Left out

- The code the release build compiles out: the debug checks (`EquivalentChains::validate`,
  `Chains::validate`, `validate_graph_and_queue`), `svg_draw_polyline_chain`, and the `#if 0` code
  (`improve_ordering_by_segment_flipping`, the four-span `minimum_crossover_cost` and
  `do_crossover`, both `reorder_by_three_exchanges_with_segment_flipping`s and the `Matrixd`
  typedef only they use).
- The chaining of extrusion entities and print instances (ShortestPath.hpp 21-28 and 48-55,
  ShortestPath.cpp 20-41, 1026-1095 and 2015-2042): layer-3's, in orca-layer, since it needs the
  entities and the print. It calls `segments`.

## How it is proven

`tests/probes.rs` replays paths-1's records of `paths.sop1` (`parity/SOP1.md`, "Probe files", and
`parity/sop1/paths.md`), which `bun run engine parity probe --only paths` writes from the C++
engine's own build (`parity/oracle/probe/paths.cpp` and `paths/`): PSLN (`chain_lines`), PSCP
(`chain_points`), PSCX (`chain_expolygons`), PSXP and PSXE (the extrusion chainings, through a
test-side model of the probe's entity types over `chain_segments_greedy` and
`chain_segments_greedy_constrained_reversals`, since the entities are layer-3's), PSPL
(`chain_polylines`) and PSUO (`union_pt_chained_outside_in`). Every point, index, flag and count is
compared exactly. The geom-1 and geom-2 kinds of the same file are orca-geom's. Every record of the
committed set (151) and of the full set (5437) matched on 2026-11-13. `tests/edges.rs` checks the
`NotPorted` stops, the one-segment comparisons and the lookup's order on hand-made inputs.

```sh
bun run rs:test -p orca-paths
SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes bun run rs:test:release -p orca-paths --test probes
```

## Dependencies

Every library crate of phase O2 and every crate of phase O3 before it in the build order:
`orca-arrange`, `orca-config`, `orca-crt`, `orca-eigen`, `orca-exact`, `orca-fmt`, `orca-geom`,
`orca-json`, `orca-mesh`, `orca-model`, `orca-placeholder`, `orca-qhull`, `orca-stl`, `orca-tbb`,
`orca-xml`, `orca-zip`, `orca-index`. `orca-probe` for its tests. Nothing from crates.io. paths-1
calls orca-index (the KD tree, the mutable priority queue, `NotPorted`), orca-geom (points,
polylines, polygons, ExPolygons, Clipper's `union_pt` tree), orca-eigen (`Vec2d`), orca-stl
(`sort_by`, `min`, `max`) and orca-crt (`fabs`, `f64_to_i64`).

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.
