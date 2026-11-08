# orca-qhull

Qhull 2015.2 (libqhull_r 7.2.0 and libqhullcpp), as OrcaSlicer builds it from deps_src/qhull.

The C++ engine links Qhull 2015.2 as `qhullstatic`, built from OrcaSlicer's `deps_src/qhull` (`src/libqhull_r` and `src/libqhullcpp`; `global_r.c` names it qhull_r 7.2.0, and the engine's build log says so), and runs it in one place: `its_convex_hull` (`TriangleMesh.cpp`) calls `orgQhull::Qhull`'s `runQhull("", 3, n, points, "Qt")`. libslic3r reaches it through `TriangleMesh::convex_hull_3d`, which `ModelVolume::calculate_convex_hull`, the auto-orientation of `Orient.cpp` and SLA's `Rotfinder.cpp` call. This crate is that run, translated statement by statement by the o2-mesh task of phase O2 (2026-11-07): [`convex_hull`] takes the points as `double`s and returns the facets as `QhullFacetList` walks them (each facet's id, its vertices by id and input point, and its normal) with the messages `~Qhull` writes to `std::cerr`, or the code of the error Qhull stopped on, which the C++ throws as a `QhullError`. `orca-mesh`'s `its_convex_hull` walks the facets into triangles.

What the run does: `qh_initflags` sets only `Qt` (triangulate); `qh_initqhull_globals` adds Qhull's default merging for a 3-d hull (`C-0`: before each point's new facets join the hull, the ones that are not clearly convex merge, by their centrums, with a margin of twice the rounding bound `DISTround`), the check of the outer planes and the near-inside points. `qh_qhull` builds the hull a point at a time (the initial simplex, then the furthest point of each facet's outside set, its visible facets and horizon, the cone of new facets matched by a hash table of their ridges, the merges, the partition of the points left), and `qh_prepare_output` triangulates the merged facets. No option of the run joggles, rotates, scales or draws a random number after `qh_initqhull_globals`' check of the generator (`hull.rs`).

The modules:

- `set`: Qhull's sets (`qset_r.c`) over arena indices, with C's two lengths: the size counts a NULL element, a `FOREACH` stops at it.
- `qh`: the data structures (`facetT`, `vertexT`, `ridgeT`, `mergeT`, `qhT`) and the facet and vertex lists.
- `geom`: distances, hyperplanes, centrums, angles and the searches for the best facet (`geom_r.c`, `geom2_r.c`).
- `poly`: the initial simplex, the new facets and their matching, the vertex neighbours, the check of the outer planes and the triangulation (`poly_r.c`, `poly2_r.c`).
- `libqhull`: the build and the partition of the points (`libqhull_r.c`).
- `merge`: the merges (`merge_r.c`): coplanar horizons, degenerate and redundant facets, the non-convex ridges sorted by angle, and the vertices merging leaves.
- `hull`: the set-up, the run and its result (`global_r.c`, `libqhullcpp`'s `Qhull.cpp`).

How it is written:

- `realT` is `double`, each expression is evaluated in the C's order, and the square roots go through `orca-crt`. Qhull's `qsort`s (the new merges by `qh_compareangle`, which never returns 0, in `qh_getmergeset` and `qh_getmergeset_initial`; the vertices by `qh_comparevisit`, an unsigned difference, in `qh_find_newvertex`) are `orca-crt`'s `qsort`, the static UCRT's own, so equal keys land where the C's leave them.
- Facets, vertices and ridges live in arenas in the order Qhull makes them, so an index is an id, and a freed object stays unused. Qhull's memory pool, its statistics and its trace output change no result and are left out, but for the two counters the build reads (`Ztotmerge`, `Zsetplane`).
- What the options never reach is left out where it stands, with the option named. The few branches of reached functions that they never take return `Error::Unported`; none returns it on the probe's sets.
- Messages: libqhullcpp's `qh_fprintf` appends every message to the run's text (`QhullQh`), at most 2999 bytes each (`vsnprintf` into `MSG_MAXLEN` bytes). An error ends the run, and the C++ throws the text away with the `QhullError`, so the translation returns only the error's code. After a hull that succeeded, the only message the run can leave is the narrow-hull warning (9375; the others are traces, options' or errors'), printed here with `orca-fmt`'s `printf`, which `~Qhull` writes to `std::cerr` after `"\nQhull output at end\n"`.

Three places hang on more than the inputs in the C++ (`parity/HAZARDS.md`, orca-qhull): `qh_gethash` adds the addresses of vertices, which here are their ids, and only a ridge that more than two new facets share (a duplicate ridge) is resolved in hash-table order; a non-simplicial facet whose triangles are all degenerate leaves them its freed normal; a normal that is NaN in all three coordinates makes `qh_normalize2` dereference NULL. The probe's sets reach none of the three.

## Probe gate

`tests/probes.rs` replays the QHUL records of `qhull.sop1` (`parity/SOP1.md`), which `bun run engine parity probe --only qhull` writes from the C++ engine's own build (`parity/oracle/probe/qhull.cpp`): the committed subset in `parity/probes/` by default, the full set with `SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes`. Each record holds a point set and what `its_convex_hull` made of it: the vertices, the triangles, the count of face properties and the text on `std::cerr`. The test runs [`convex_hull`] and the walk of `its_convex_hull` (repeated in the test, as `orca-mesh` depends on this crate; `orca-mesh`'s own probe test replays the same records through its `its_convex_hull`), compares every field bit for bit, and fails on a run that stops on a branch the translation leaves out (`Error::Unported`), which an empty record would hide. The sets: too few, flat, collinear, repeated and non-finite points, a tetrahedron, a flat one that warns, a cube, a grid; seeded points in boxes, balls, slabs, on spheres, grids and box faces, with duplicates, far from the origin; tiny and huge coordinates, needles and nearly flat box faces; lattice sets whose slanted faces hold exactly coplanar points; clusters with points 10^6 to 10^11 away, whose facets are flat to Qhull's precision and merge by their centrums; prisms, cylinders and cones; and the shared vertices of OrcaSlicer's five readable test STL files. Every record of both sets matched on 2026-11-07: 3619 point sets in the full set (36 that Qhull refuses, 11 that warn of a narrow hull) and 199 in the committed one.

What the full set does not reach, found by counting calls in a scratch copy of the crate on 2026-11-07: a cycle of more than one new facet merging into one coplanar horizon facet (`qh_mergecycle` with `qh_mergecycle_facets`, `_neighbors`, `_ridges`, `_vneighbors` and `qh_basevertices`), a duplicate ridge (`qh_matchduplicates`, `qh_addhash`, `qh_check_dupridge` with `qh_outerinner` and `qh_maxouter`), the mirrored and the all-degenerate triangles of `qh_triangulate` (`qh_triangulate_mirror`, the `MRGmirror` branch of `qh_appendmergeset`), the fallbacks of the best-facet search (`qh_findbestlower`, `qh_nearvertex`, `qh_findfacet_all`), `qh_furthestout` (a narrow hull whose facet lost track of its furthest point), `qh_copynonconvex`, `qh_degen_redundant_facet`, the drop of a vertex that is in none of the ridges in `qh_find_newvertex`, and a normal too small to divide by in `qh_normalize2` (`qh_maxabsval`, where an all-NaN normal dereferences NULL). A search of 300,000 further sets (small integer points, near-duplicate twins, precision-flat lattices and box faces) reached none of them either. They are translated from the source as the rest is, and no record holds them yet.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only; the code
it carries keeps its own licence, which `NOTICE` names in the crate's own section.

## Dependencies

Declared up front, before the crate is filled (o2-geom, 2026-11-07), so that the tasks that fill the O2 crates in parallel never change `Cargo.toml` or `Cargo.lock`:

- `orca-crt`: `sqrt` and `qsort`
- `orca-stl` (declared; the run sorts nothing but through `qsort`)
- `orca-fmt`: `printf` for the narrow-hull warning
- `orca-probe` (development only: the probe gate)
