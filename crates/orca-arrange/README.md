# orca-arrange

Arrangement: libnest2d, Arrange.cpp, NLopt 2.5.0 and Boost.Geometry's R*-tree.

Arrangement as the C++ engine places objects on the bed: libnest2d (`deps_src/libnest2d`, with libslic3r's geometry and NLopt as its optimiser), libslic3r's `Arrange.cpp`, NLopt 2.5.0's subplex and Nelder-Mead (`sbplx`, `nldrmd`, `redblack`, `qsort_r`, `stop` and the API) and Boost.Geometry's R*-tree (Boost 1.84.0's `geometry/index`), each as OrcaSlicer v2.4.2 builds it. Filled by the o2-arrange task on 2026-11-07.

- `arrange` (`src/arrange.rs`): `Slic3r::arrangement::arrange` on a box, a circle, a polygon or an infinite bed, from bed points or a typed bed, with fixed items, excluded regions, inflation, print by object and Orca's object function; the results are written back to the `ArrangePolygon`s as the C++ writes them.
- `firstfit` (`src/firstfit.rs`): libnest2d's first-fit selection as Orca changed it: plates scored together, fixed items preloaded on their plates, unfit items marked, and the placers' vector grown as MSVC grows it.
- `placer` (`src/placer.rs`): the no-fit-polygon placer, its edge cache, `minimizeCircle` and `finalAlign`; the corner searches and no-fit polygons run in parallel through `orca-tbb`, each in its own slot.
- `nfp` (`src/nfp.rs`): the convex no-fit polygon, whose edge sort compares `boost::rational<int128_t>` values from `orca-exact`.
- `item`, `geometry` (`src/item.rs`, `src/geometry.rs`): libnest2d's item and geometry traits over libslic3r's types, with libnest2d's own hull and 64-bit integer products.
- `optimizer`, `nlopt` (`src/optimizer.rs`, `src/nlopt.rs`): `TOptimizer<L_SUBPLEX>` through `nlopt::opt`, and NLopt's subplex and Nelder-Mead with its red-black tree and its own `qsort_r`.
- `bg`, `rtree` (`src/bg.rs`, `src/rtree.rs`): the Boost.Geometry predicates libnest2d's backend calls, and `Arrange.cpp`'s spatial index for the queries it makes.

What the C++ throws is an `ArrangeError` (or a panic carrying a `ClipperException`, where Clipper throws). Where the C++ ends the process, in `~_NofitPolyPlacer`'s `finalAlign` (a triangle alone on a plate), this crate panics; where it reads past a vector, it stops with `ArrangeError::Undefined`; `allow_rotations`, whose rotating calipers are not translated, gives `ArrangeError::RotationsNotTranslated`. `parity/HAZARDS.md` (orca-arrange) lists these paths, the clocks NLopt reads and the inputs the probe keeps to.

## Probe gate

`tests/probes.rs` replays the NLOP, NOPT, RTRE, NFPC, NGEO and ARNG records of `arrange.sop1` (`parity/SOP1.md`), which `bun run engine parity probe --only arrange` writes from the C++ engine's own libnest2d, `Arrange.cpp` and NLopt: NLopt and libnest2d's optimiser evaluation by evaluation, the spatial index, no-fit polygons, items, hulls, predicates and edge caches, and 313 whole arrangements with every callback, compared bit for bit. The committed subset in `parity/probes/` runs by default, the full set with `SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes`. It also checks that every kind of the file has a replay here or in `orca-exact`, which replays XI28 and XRAT.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only; the code
it carries keeps its own licence, which `NOTICE` names in the crate's own section.

## Dependencies

Declared up front, before the crate is filled (o2-geom, 2026-11-07), so that the tasks that fill the O2 crates in parallel never change `Cargo.toml` or `Cargo.lock`:

- `orca-crt`
- `orca-stl`
- `orca-tbb`
- `orca-fmt`
- `orca-eigen`
- `orca-geom`
- `orca-exact`
- `orca-probe` (development only: the probe gate)
