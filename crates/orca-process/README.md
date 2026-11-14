# orca-process

Phase O3 of the Rust translation: libslic3r's Print::validate and Print::process as the command line
runs them, up to the layers answer.

Set up empty by orca-o3-skeleton on 2026-11-13: a manifest that declares every dependency the crate
will need, so that no unit that fills it changes `Cargo.toml` or `Cargo.lock`, and a `src/lib.rs`
with the header and the crate's doc comment. Each piece below adds its modules and its tests and
keeps this README true.

## The pieces that fill it

| Piece | What it adds |
| --- | --- |
| `orca-o3-process-1` | Validate and the CLI path up to process() |
| `orca-o3-process-2` | Print::process and the layers answer |

The C++ they translate (under `src/libslic3r/` of OrcaSlicer v2.4.2 unless named): Print.cpp,
Model.cpp, GCode.cpp, GCode/WipeTower.cpp, GCode/WipeTower2.cpp, PrintObject.cpp, Thread.cpp.

## How it is proven

No probe file of its own: the step-boundary dumps the oracle writes (`parity/SOP1.md`, "Files") and
`parity certify` hold it, compared byte for byte with the Rust program's dumps.

## Dependencies

Every library crate of phase O2 and every crate of phase O3 before it in the build order:
`orca-arrange`, `orca-config`, `orca-crt`, `orca-eigen`, `orca-exact`, `orca-fmt`, `orca-geom`,
`orca-json`, `orca-mesh`, `orca-model`, `orca-placeholder`, `orca-qhull`, `orca-stl`, `orca-tbb`,
`orca-xml`, `orca-zip`, `orca-index`, `orca-paths`, `orca-voronoi`, `orca-slice`, `orca-layer`,
`orca-arachne`, `orca-print`, `orca-perimeter`, `orca-surface`, `orca-fill`, `orca-support`,
`orca-plate`. `orca-probe` for its tests. Nothing from crates.io.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.
