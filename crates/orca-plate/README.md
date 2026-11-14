# orca-plate

Phase O3 of the Rust translation: libslic3r's plate-wide steps: ToolOrdering for one filament, the
wipe tower's data, the simplify steps, skirt and brim, the conflict checker, the fake wipe tower,
the layers to print, and the oracle's print dump writer.

Set up empty by orca-o3-skeleton on 2026-11-13: a manifest that declares every dependency the crate
will need, so that no unit that fills it changes `Cargo.toml` or `Cargo.lock`, and a `src/lib.rs`
with the header and the crate's doc comment. Each piece below adds its modules and its tests and
keeps this README true.

## The pieces that fill it

| Piece | What it adds |
| --- | --- |
| `orca-o3-plate-1` | ToolOrdering for one filament, the wipe tower's data and the simplify steps |
| `orca-o3-plate-2` | Skirt and brim |
| `orca-o3-plate-3` | The conflict checker, the fake wipe tower and the layers to print |
| `orca-o3-dump-2` | The oracle's print dump writer |

The C++ they translate (under `src/libslic3r/` of OrcaSlicer v2.4.2 unless named):
GCode/ToolOrdering.hpp and .cpp, Print.hpp, Print.cpp, PrintObject.cpp, LayerRegion.cpp, Layer.cpp,
ParameterUtils.hpp and .cpp, Brim.hpp and .cpp, Model.cpp, GCode/ConflictChecker.hpp and .cpp,
GCode.cpp, GCode/GCodeProcessor.hpp, and the oracle's OracleDump.cpp.

## How it is proven

No probe file of its own: the step-boundary dumps the oracle writes (`parity/SOP1.md`, "Files") and
`parity certify` hold it, compared byte for byte with the Rust program's dumps.

## Dependencies

Every library crate of phase O2 and every crate of phase O3 before it in the build order:
`orca-arrange`, `orca-config`, `orca-crt`, `orca-eigen`, `orca-exact`, `orca-fmt`, `orca-geom`,
`orca-json`, `orca-mesh`, `orca-model`, `orca-placeholder`, `orca-qhull`, `orca-stl`, `orca-tbb`,
`orca-xml`, `orca-zip`, `orca-index`, `orca-paths`, `orca-voronoi`, `orca-slice`, `orca-layer`,
`orca-arachne`, `orca-print`, `orca-perimeter`, `orca-surface`, `orca-fill`, `orca-support`.
`orca-probe` for its tests. Nothing from crates.io.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.
