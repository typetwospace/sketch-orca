# orca-mesh

Triangle meshes: admesh's reading and repair, and libslic3r's TriangleMesh with its transforms.

Triangle meshes as the C++ engine reads and repairs them: admesh (`deps_src/admesh`) and libslic3r's `TriangleMesh` (`TriangleMesh.cpp`, `MeshSplitImpl.hpp`), translated by the o2-mesh task of phase O2 (2026-11-07). The modules:

- `admesh`: the STL reader (`stlinit`), the facet connectivity and the removal of unconnected facets (`connect`), the facet orientation and normals (`normals`), the facets turned into shared vertices (`shared`), and the checks, measures and transforms of `util.cpp` (`util`). admesh reads files with the C runtime's stdio (`fopen`, `fread`, `fgets`, `fscanf`), which `orca-fmt`'s `stdio` and `scanf` modules reproduce; what it prints (`stl_write_facet`) goes to `orca-fmt`'s C `stdout`.
- `triangle_mesh`: `TriangleMesh` and its statistics: `ReadSTLFile` with the repair on import (`trianglemesh_repair_on_import`), the statistics taken from the indexed set, the scale, translate, rotate, mirror and transform calls, the bounding boxes, `split`, `merge` and the 2D convex hull.
- `its`: the `its_*` functions those reach: the volume, the bounding box, the face neighbours (`create_face_neighbors_index`, sequential and as the oneTBB loop through `orca-tbb`), the patches and the split, the transforms, and the projections behind the 2D hulls.
- `eigen_lu`: the determinant Eigen takes of a 3x3 block of runtime size, by partial-pivoting LU (MPL-2.0, as `NOTICE` says).
- `log`: the engine's `BOOST_LOG_TRIVIAL` messages, word for word, to a sink the caller sets.

How a file is read, step by step, is in `src/admesh/stlinit.rs`; how the edges are matched, in `src/admesh/connect.rs`. Floats stay `f32` where the C++ has `float`, and every mixed `float` and `double` expression keeps the C++ promotions. One file the C++ engine cannot read gets a defined answer: an ASCII STL whose `solid` line is longer than 255 bytes (any file with CR line ends) overruns admesh's 256-byte buffer and kills the engine, and `stl_open` refuses it with `StlError::OverlongSolidLine` (`parity/CORPUS.md`, "An engine fault: 20mmbox-CR.stl"; `tests/stl_open.rs`). The other places where the C++ reads out of bounds or races are in `parity/HAZARDS.md`, orca-mesh. `TriangleMesh::convex_hull_3d` and `its_convex_hull` come with `orca-qhull`.

## Probe gate

`tests/probes.rs` replays the records of `mesh.sop1` (`parity/SOP1.md`, MCAS and MSTD), which `bun run engine parity probe --only mesh` writes from the C++ engine's own build: the committed subset in `parity/probes/` by default, the full set with `SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes`. Each record is rebuilt from the Rust byte for byte, with the inputs it holds, and compared whole: admesh's state after `stl_open` and after each repair step (as hashes in MCAS, whole in MSTD), the shared vertices, then `ReadSTLFile`'s mesh, its measures, split and 2D hull, and a seeded chain of transforms with each result. The replay runs as the oracle does, one thread in an arena of concurrency 8 (`orca_tbb::Arena::serial(8)`), since the face neighbours of `split` and `is_splittable` race between threads in the C++. Every record of both sets matched on 2026-11-07. The stdio records (`cstdio.sop1`) are `orca-fmt`'s.

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
- `orca-qhull`
- `orca-probe` (development only: the probe gate)
