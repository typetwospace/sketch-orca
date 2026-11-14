# orca-print

Phase O3 of the Rust translation: libslic3r's Print and PrintObject: their steps' state and
invalidation, Print::apply, and PrintObject::slice with the elephant foot compensation and the
polyholes.

Set up empty by orca-o3-skeleton on 2026-11-13: a manifest that declares every dependency the crate
will need, so that no unit that fills it changes `Cargo.toml` or `Cargo.lock`, and a `src/lib.rs`
with the header and the crate's doc comment. Each piece below adds its modules and its tests and
keeps this README true.

## The pieces that fill it

| Piece | What it adds |
| --- | --- |
| `orca-o3-print-1` | Print's and PrintObject's state, invalidation and the polyholes' set-up |
| `orca-o3-print-2` | Print::apply |
| `orca-o3-print-3` | PrintObject::slice with the elephant foot and the polyholes |

The C++ they translate (under `src/libslic3r/` of OrcaSlicer v2.4.2 unless named): Print.cpp,
PrintObject.cpp, PrintApply.cpp, PrintObjectSlice.cpp, ElephantFootCompensation.hpp and .cpp.

## print-1: state, invalidation and polyholes (2026-11-14)

orca-layer declares `Print` and `PrintObject`; this crate adds their members as free functions.
A print object's back-pointer to its print is an argument: `PrintInherited` (the print's `base`,
its steps, model copy and cancel callback) where a call changes the print's steps, the `Print`
where it reads the print's own members.

| Module | What it carries |
| --- | --- |
| `print` | Print.cpp 1-92, 423-448, 450-612, 3290-3302, 3385-3388, 3851-3885: `PrintRegion`'s constructors from a config, `clear`, `has_tpu_filament`, `set_calib_params`, `is_step_done` of an object step, the extruder queries, `num_object_instances`, `max_allowed_layer_height`, `print_object_ids`, the skirt and brim queries, `layers_sorted_for_object`, `has_wipe_tower`, `enable_timelapse_print`, the shrinkage compensation checks |
| `print_object` | PrintObject.cpp 84-158, 1022-1089, 3835-3859, 4676-4725 and Print.cpp 5136-5169: the constructor, the destructor's record, `set_instances`, `all_regions`, the layers and support layers, the layer queries, PrintInstance's and FuzzySkinPaintedRegion's members |
| `invalidation` | Print.cpp 93-421, 428-435 and PrintObject.cpp 1091-1509: which steps a changed option invalidates (the C++'s `if ... else if` chains as tables in source order, first branch wins) and how a step reaches its dependants |
| `polyholes` | PrintObject.cpp 160-186: `create_polyholes` |

The extruder queries (Print.cpp 450-547) are orca-o3-process-1's lines, translated here because
`max_allowed_layer_height`, the shrinkage checks and `Print::apply` call `extruders()` and
orca-process comes after this crate.

Held beside the print, by the crate that owns their types: the tool ordering, the wipe tower data,
the extruder statistics, the fake wipe tower and the conflict result (orca-plate's), the octrees
and the tree-support preview cache (phase O5's), the G-code processor result (phase O4's).
`has_tpu_filament` takes the tool ordering's filaments as an argument.

The stops (`NotPorted`):

- `PrintObject::alloc_tree_support_preview_cache` (tree-supports): only tree supports allocate the
  cache, whose type is phase O5's.
- `Print::extruders(true)` with more than one filament (multi-filament): it reads the plate's
  custom G-code tool changes, which orca-model does not hold. With one filament the command line
  makes none, so nothing is added.
- Inputs the C++ has no answer for: a print object made with no instance, a height range whose
  `extruder` is not an int, a fuzzy-skin region whose parent index or region is out of reach, a
  config without the typed option an invalidation branch reads.

The destructor's info record names addresses, which never match the C++'s; info records are below
the command line's level.

## How it is proven

No probe file of its own: the step-boundary dumps the oracle writes (`parity/SOP1.md`, "Files") and
`parity certify` hold it, compared byte for byte with the Rust program's dumps.

print-1's unit tests check the invalidation (each special branch, first-branch-wins, the
propagation), `set_instances`' statuses, the constructor's centring, the layer queries and the
polyholes. `tests/invalidation.rs` reads the work folder's Print.cpp and PrintObject.cpp at run
time and fails while the tables differ from the C++'s lists of keys and steps.

## Dependencies

Every library crate of phase O2 and every crate of phase O3 before it in the build order:
`orca-arrange`, `orca-config`, `orca-crt`, `orca-eigen`, `orca-exact`, `orca-fmt`, `orca-geom`,
`orca-json`, `orca-mesh`, `orca-model`, `orca-placeholder`, `orca-qhull`, `orca-stl`, `orca-tbb`,
`orca-xml`, `orca-zip`, `orca-index`, `orca-paths`, `orca-voronoi`, `orca-slice`, `orca-layer`,
`orca-arachne`. `orca-probe` for its tests. Nothing from crates.io.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.
