# orca-layer

Phase O3 of the Rust translation: libslic3r's extrusion entities and their collections, Flow and
variable width, Surface and its collections, Layer and LayerRegion, the declarations of Print,
PrintObject and PrintRegion, and the reader and replay of the oracle's step-boundary dumps.

Set up empty by orca-o3-skeleton on 2026-11-13: a manifest that declares every dependency the crate
will need, so that no unit that fills it changes `Cargo.toml` or `Cargo.lock`, and a `src/lib.rs`
with the header and the crate's doc comment. Each piece below adds its modules and its tests and
keeps this README true.

## The pieces that fill it

| Piece | What it adds |
| --- | --- |
| `orca-o3-layer-1` | Extrusion entities, Flow and variable width (done 2026-11-13) |
| `orca-o3-layer-2` | Surface and the layer and print declarations (done 2026-11-14) |
| `orca-o3-layer-3` | Print's declaration, entity chaining, Layer and its regions, and the support flows (done 2026-11-14) |
| `orca-o3-dump-1` | The oracle's object and front-config dump writer, the record reader and the replay module (done 2026-11-14) |

The C++ they translate (under `src/libslic3r/` of OrcaSlicer v2.4.2 unless named):
ExtrusionEntity.hpp and .cpp, ExtrusionEntityCollection.hpp and .cpp, Flow.hpp and .cpp,
VariableWidth.hpp and .cpp, Surface.hpp and .cpp, SurfaceCollection.hpp and .cpp, Layer.hpp,
Layer.cpp, LayerRegion.cpp, PrintBase.hpp, PrintBase.cpp, Print.hpp, Print.cpp, PrintRegion.cpp,
ShortestPath.hpp and .cpp, PlaceholderParser.cpp's `update_timestamp` and `update_user_name`,
calib.hpp's `CalibMode` and `Calib_Params`, Boost 1.84's `libs/filesystem/src/path.cpp` (the path
functions PrintBase.cpp calls, BSL-1.0), and the oracle's OracleDump.hpp and OracleDump.cpp.

## The modules

layer-1 wrote these, statement by statement, with `f32` wherever the C++ has `float`:

- `src/role.rs`: `ExtrusionRole`, `ExtrusionLoopRole` and their predicates, `role_to_string` and
  `string_to_role` (ExtrusionEntity.hpp and .cpp).
- `src/path.rs`: `ExtrusionPath` and its kinds, contoured (z offsets per point), sloped (a slope at
  each end) and oriented (never reversed). Clipping, simplifying, arc fitting, the polygons a path
  covers by its width or spacing, and its volume.
- `src/multi_path.rs`: `ExtrusionMultiPath`, paths joined end to end.
- `src/extrusion_loop.rs`: `ExtrusionLoop` (orientation, splitting at a vertex or the nearest
  point, clipping the end, overhangs, smoothness) and `ExtrusionLoopSloped` (a loop cut into a
  sloped start, a flat middle and a sloped end for the scarf seam). `prev_idx_modulo` and
  `next_idx_modulo` of Utils.hpp are translated here, as orca-slice does with its own.
- `src/entity.rs`: `ExtrusionEntity` as one enum of the eight classes, `ExtrusionEntitiesPtr` as a
  vector of it, and the append helpers of ExtrusionEntity.hpp. `Clone` is the C++'s virtual
  `clone()`, which slices: a sloped path clones as a plain path, a sloped loop as a plain loop.
- `src/collection.rs`: `ExtrusionEntityCollection` and `filter_by_extrusion_role`, with the body
  of ExtrusionEntityCollection.cpp (its chaining runs layer-3's `chain`).
- `src/flow.rs`: `Flow` and its errors (Flow.hpp and .cpp; layer-3 added the support flows).
- `src/variable_width.rs`: `variable_width` and `thick_polyline_to_multi_path`, the paths of
  Arachne's thick polylines, split where the width changes past a tolerance.
- `src/cpp_vec.rs`: what MSVC's `std::vector` does to a path's `inset_idx` as it moves the path.
  A path's copy and move constructors set `inset_idx` to -1 (as a multi path's and a collection's
  do; a loop's implicit copy keeps it) and its assignment keeps the target's, so a vector that
  grows gives every element -1, while erase, insert with room to spare and reverse keep the insets
  by slot. `push`, `insert`, `extend_from_slice`, `erase`, `reverse` and `assign`
  do the same to a `Vec`, and every vector of paths the C++ grows goes through them.

layer-2 wrote the data model's declarations, with the inline bodies of the headers:

- `src/surface.rs`: `SurfaceType`, `Surface` and its predicates, and the helpers over lists of
  surfaces (Surface.hpp and .cpp, but the SVG output, which only debugging builds call). An empty
  contour's box is the oracle build's (see `get_extents`).
- `src/surface_collection.rs`: `SurfaceCollection` (SurfaceCollection.hpp and .cpp). `group`
  gives lists of references into the collection; the filters that keep or drop by type compact in
  place by swapping, as the C++ does, so the kept surfaces keep their order.
- `src/layer.rs`: `LayerRegion`, `Layer`, `SupportLayer` and their small types (Layer.hpp), with
  Line.hpp's `CurledLine`, which belongs in orca-geom. The methods with bodies in Layer.cpp,
  LayerRegion.cpp and the support code are layer-3's; the module header lists them.
- `src/print_base.rs`: the step states (`PrintState`, `StateWithTimeStamp`, `Warning`), the
  status and cancel callbacks, `PrintBase`, `PrintBaseWithState` and `PrintObjectBaseWithState`
  (PrintBase.hpp), with the config of the `PlaceholderParser` that `PrintBase` holds, which belongs
  in orca-placeholder. `set_done` and `active_step_add_warning` of the two bases with state call
  PrintBase.cpp's `status_update_warnings`, so they are layer-3's with PrintBase.cpp.
- `src/print_region.rs`: `PrintRegion`. The constructors from a config alone and the methods with
  bodies in PrintRegion.cpp are layer-3's.
- `src/print_object.rs`: Print.hpp up to `PrintObject`: `PrintStep` and `PrintObjectStep`,
  `PrintInstance`, `PrintObjectRegions` and its parts, and `PrintObject` with its inline accessors.

layer-3 finished the data model:

- `src/print.rs`: `PrintStatistics` and `Print` (Print.hpp 800-867 and 881-1232) with its inline
  members, and Print.cpp's `get_filament_maps`, `get_filament_map_mode`, `get_extruder_id` and
  the extruders' printable heights and areas. `Print` holds its `PrintBaseWithState` as `base` and
  dereferences to it; its print regions are `PrintRegionPtr`s (the shared regions' `Arc` and an
  index). The tool ordering, wipe tower data, extruder statistics, conflict result and fake wipe
  tower are orca-plate's types (phase O4), held beside the print; the members with bodies in
  Print.cpp and the rest belong to the crates that carry those bodies (the module header lists
  them). `has_brim` (Print.cpp 588-591, orca-print's) and calib.hpp's `CalibMode` and
  `CalibParams` (the calibration code, orca-gcode's) are translated here because the print needs
  them now.
- `src/chain.rs`: the entity and instance parts of ShortestPath: `chain_extrusion_entities`,
  `reorder_extrusion_entities`, `chain_and_reorder_extrusion_entities` (which first drops the
  entities without end points), the same three for paths, and `chain_print_object_instances`.
  The greedy chaining itself is orca-paths'. A path moved into the reordered vector loses its
  inset, as MSVC's move does (`cpp_vec`).
- `src/fs_path.rs`: what PrintBase.cpp takes from Boost.Filesystem's path, version 3: `filename`,
  `extension`, `replace_extension`, `append` (`operator/`) and `make_preferred`, with Windows'
  separators and root names on Windows. It belongs in orca-model, whose `utils` translates
  `filename_v3` privately.
- `src/print_base.rs` gained PrintBase.cpp: `set_status`, `status_update_warnings` (both),
  `update_object_placeholders`, `output_filename`, `output_filepath`, `PrintTryCancel`, and the
  `set_done` and `active_step_add_warning` of both bases with state; with PlaceholderParser's
  `update_timestamp` and `update_user_name`, which belong in orca-placeholder.
- `src/print_region.rs` gained PrintRegion.cpp: `extruder`, `flow`, `nozzle_dmr_avg`,
  `bridging_height_avg` and both `collect_object_printing_extruders`.
- `src/layer.rs` gained the base parts of Layer.cpp (`empty`, `add_region`, `make_slices`, the
  untyped slices' backup and restores, `merged`, `get_sparse_infill_max_void_area`,
  `get_extruder_id`) and of LayerRegion.cpp (`flow`, `bridging_flow`,
  `slices_to_fill_surfaces_clipped`, `get_extents`). ClipperUtils' `offset` of a list of surfaces
  is translated there; it belongs in orca-geom.
- `src/flow.rs` gained the support flows (Flow.cpp 232-272): `support_material_flow`,
  `support_transition_flow`, `support_material_1st_layer_flow` and
  `support_material_interface_flow`.

dump-1 wrote the oracle's dumps and their replay:

- `src/dump.rs`: the parity oracle's OracleDump.hpp and the object and front-config parts of
  OracleDump.cpp (sketch-orca oracle patches 0005 to 0007; `parity/SOP1.md`). `Payload` and `Dump`
  build a SOP1 file (the magic, version 1, HEAD and RUN, the records, END with their count); the
  `put_*` writers write geometry, extrusion entities (an entity's kind is its variant, its
  `can_reverse` the virtual one) and configs (`config_record`). `dump_object_step` writes
  `object<i>-<step>-<n>.sop1` (OBJ, LAYR, SUPL, OSTA, OWRN, LEXT) and `dump_front_config` writes
  `front-config-<n>.sop1` (the CONF records, FPRT, FOBJ), only while `SKETCH_ORCA_ORACLE_DUMP`
  names a folder. RUN reports the knobs, the arena's concurrency, the thread limit and the FMA3
  state; `RunState` holds them so that a replay writes a dump again under its recorded RUN. FPRT's
  wipe tower and skirt answers are Print.cpp's (orca-print's): the caller passes them in
  `PrintAnswers`. Translated here though other crates own them: `GlobalControl` and
  `global_control_active_value` (TBB's `global_control`, orca-tbb's) and
  `shift_without_plate_offset` (Print.cpp 5147-5152, orca-print's). The print records are
  orca-o3-dump-2's and the front-objects dump is orca-cli's (`oracle.rs`, which still holds its
  own copies of `Payload`, the `put_` helpers and `config_record` until orca-o3-process-1 makes it
  use these).
- `src/replay/`: what later pieces use to prove their steps against the oracle. `mod.rs` finds the
  goldens' runs (`cases/<case>/<run>/dumps/` under `SKETCH_ORCA_GOLDENS_DEEP`), reads dump names
  and compares two dumps byte for byte, naming the first record that differs
  (`first_difference`). `sop1.rs` reads the container and a payload, failing with a
  `ReplayError` that says where. `records.rs` reads every record kind of every dump (the print
  dumps' PRNT to TLXT included) into its kind and named fields as written. `state.rs` reads the
  front-objects dump into a `Model` (with the run's config, the plates' custom G-code as its record
  and the brim ears beside it), the front-config dump into a `Print` over that model, and an object
  dump into the print's object; `write_front_config` and `write_object_dump` write them again.

What a dump does not hold, the replay rebuilds as the C++ has it: a region's config hash, a volume
region's box, and the step states (by the state changes that leave the warnings as recorded). A
collection with `no_sort` keeps `is_reverse` true (its `can_reverse` is false either way). A layer
region's region is the first region of its layer's object with the recorded print region id; a
layer's object is the print object whose layers it is, the shared object for an object that shares
another's layers (`dump::layer_object`: `copy_layers_from_shared_object` copies the C++'s layer
pointers). A volume region's `prev_same_region` is none, as `Print::apply` leaves it. The shared
regions' `cached_volume_ids`, which no dump holds, are left empty: only `Print::apply`
(PrintApply.cpp) reads them, no step of process().

The C++ back-pointers become indices or arguments: a layer region's region is an index into its
object's `PrintObjectRegions::all_regions` (`LayerRegion::region` takes the object); a layer's
upper and lower layers are `Option<usize>` into the object's layers; a print object's model object
and an instance's model instance are indices into the print's model; the volume regions' parent,
model volume and region are indices; `shared_object` is `Option<usize>`. The print objects of one
model object share their regions through an `Arc<PrintObjectRegions>` in place of the C++'s
manual count, so `PrintRegion`'s print index and reference count, which `Print::apply` changes
while the regions are shared, are atomics. `PrintBase`'s state mutex is left out: a CLI run has no
UI thread. `throw_if_canceled` panics with a `CanceledException` payload, which a CLI run never
reaches. A method that reads its object's print (a region's or a layer region's flows, a layer's
void area) takes the object and the print as arguments.

## What stops, and why

The calls below stop with orca-index's `NotPorted` where the C++ reads outside a list (no answer in
its release build); domain A never reaches them:

- `first_point` and `last_point` of an empty path, of a multi path or loop with no paths, and of a
  collection with no entities; `ExtrusionLoop::polygon` and `ExtrusionMultiPath::as_polyline` over
  an empty path.
- `ExtrusionLoop::split_at` when the nearest path has fewer than two points, and
  `ExtrusionLoop::is_smooth` on a loop with no points.
- `Flow::extrusion_width` when the key has no float-or-percent option, or `nozzle_diameter` is an
  empty list.
- `PrintState::active_step_add_warning` with no step active (the C++ asserts, then reads
  `m_state[-1]`).
- `PrintObject::num_printing_regions` and `printing_region` on an object without regions (the
  C++ dereferences a null `m_shared_regions`).
- `Layer::make_slices` on an island with an empty contour, and
  `LayerRegion::slices_to_fill_surfaces_clipped` on a slice of type `stCount` (the C++ indexes
  past its array).
- A flow that reads `nozzle_diameter` (`PrintRegion::flow`, `nozzle_dmr_avg`,
  `LayerRegion::bridging_flow`, the support flows) when it is an empty list.

The replay stops with `NotPorted` on a front-objects dump with painted facets (PNTV's
`triangles_to_split` not empty: the painted-facets family, outside domain A), as orca-model's
`TriangleBitStreamMapping` has no constructor yet.

Two stops wait for later phases (family gcode-export):

- `PrintBase::output_filename` of a format that is not empty runs `PlaceholderParser::process`,
  the macro language, which is orca-gcode's (phase O4). With no format and no input file name
  (no object prints, no base given) the C++ dereferences a null option; that stops too.
- `PrintBase::output_filepath` with an empty path takes the name
  `Model::propose_export_file_name_and_path` proposes, which is not translated: a CLI run always
  passes a path.

## How it is proven

`tests/probes.rs` (`layer_probe_replays`) replays the orca-layer records of `layer.sop1`
(`parity/SOP1.md`, "Probe files", and `parity/sop1/layer.md`), which
`bun run engine parity probe --only layer` writes from the C++ engine's own build
(`parity/oracle/probe/layer/flow.cpp`, `extrusion.cpp`, `surface.cpp` and `print.cpp`). It
decodes each record's inputs (an entity is encoded again and must give back its bytes), runs the
translation, encodes each result as the probe's writers do and compares the bytes part by part.
Kinds: LYFL, LYFS and LYFW (Flow, its helpers and `extrusion_width`), LYRO (the roles), LYEN,
LYEM, LYEL and LYES (a path, a multi path, a loop, a sloped loop), LYEC (a collection, its
`chained_path_from` included), LYEA (the append helpers) and LYVW (variable width); layer-2's LYSU
(surfaces: predicates, area, extents, polygons, colour names and which pairs could merge) and LYSC
(`SurfaceCollection`'s operations); layer-3's LYCH (the chaining of entities, paths and a
collection with entities that have no ends), LYPC (a print case: `chain_print_object_instances`
and `has_brim`), LYPR (a region's extruders, flows for every role, height and layer kind, nozzle
averages and printing extruders), LYSF (the support flows), LYPH (the object placeholders and
output file names), LYPT (Print's filament and extruder accessors) and LYLA (a layer with regions
of its own: fill surfaces, `empty`, `make_slices`, `merged`, extents, the region flows, the void
area, the untyped slices and the extruder ids). The other kinds are orca-config's; any kind
outside those lists fails the test, as does a record count other than the probe's.

The print records need the print `Print::apply` leaves, which is orca-o3-print-2's and not
translated yet. The replay builds it from the LYPC case as apply does: the full config with the
case's settings, normalised (`normalize_fdm`, the per-variant values of the printer, the print and
the filaments), the print config from it, each object's and region's config from the defaults with
the model object's settings over them, one print object per model object and instance scale in
`std::set` order, and the instances' shifts from the record. The recorded hashes of the print's,
each object's and each region's config check that build before any result is compared. The test
carries two variant lists orca-config lacks (`printer_options_with_variant_2` and
`print_options_with_variant`) for printers with one variant, all the probe has.

What the records cannot show:

- Apply's own results in LYPC (its status, each object's size and extruders) are print-2's: the
  replay reads and counts them (36), and compares none.
- An output file name of a format that is not empty waits for `PlaceholderParser::process`
  (orca-gcode, phase O4): the replay skips the recorded result and counts it (100 of LYPH's 120
  calls). The empty format's names, and the call the probe leaves out, are compared.
- `update_timestamp`, `update_user_name` and `output_filepath` have no records (the time, the
  user and the file system); `set_status`, `status_update_warnings`, `set_done` and
  `active_step_add_warning` have none either. Unit tests check what they can. `update_timestamp`
  reads the real clock: the oracle's clock knob (`SKETCH_ORCA_ORACLE_TIME`) is orca-cli's, and no
  output reached here shows the time (an empty format's name is the input file's base).
- No record has a loop that the entity chaining reverses: `could_reverse` without its `is_loop`
  passes the replay (a mutation check on 2026-11-14). Counting every instance in `num_instances`
  fails LYPH, and keeping `make_slices`' union order fails LYLA.
- LYEL's `has_overhang_point` asks about a polygon vertex the record does not hold. When the
  record's point is a vertex the replay checks the answer for it; else it requires some vertex to
  give the recorded answer.
- Every path inset in the records is -1, so the path rules of `cpp_vec` are proven by its unit
  tests; loop insets are in the records (a mutation that resets a loop's inset on copy fails LYEL
  and LYEC).
- LYSU's empty `stInternalVoid` surface (record 1) has a box the C++ leaves uninitialised; the
  replay checks the value the oracle's build gave, which `surface::get_extents` reproduces.
- The declarations of `layer`, `print_base`, `print_region` and `print_object` have no records of
  their own; their unit tests check them, and layer-3's records (LYLA, LYPR and the rest) use them.

The committed set runs by default (`bun run rs:test -p orca-layer`: 2465 comparisons over 250
records). The full set replays with
`SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes bun run rs:test:release -p orca-layer --test probes`
(75084 comparisons over 5665 records on 2026-11-14).

`tests/replay.rs` proves dump-1 against the goldens the C++ oracle recorded (`parity/README.md`),
with `SKETCH_ORCA_GOLDENS_DEEP=<work>/parity/goldens bun run rs:test:release -p orca-layer --test
replay`. For every domain-A run (its case in `parity/corpus.json` has no family of
`MARKER_FAMILIES`, which the test mirrors and checks against `scripts/lib/parity/corpus.ts`) with
a front-config dump, it reads front-objects into a model, each front-config dump into a print over
it and each object dump into that print, writes the front-config and object dumps again and
requires the recorded bytes; and it reads every dump of the run, the print dumps included, record
by record and requires each record's kind and fields to write its payload back. On 2026-11-14:
122 runs, 122 front-config and 1451 object dumps written again byte for byte, 2055 dumps
(263840 records) read record by record. Without the variable the test prints a skip, which is no
pass. Unit tests check the payload layout, the knob, the container, the entity kinds and their
`can_reverse`, the config record, the thread limit, the step-state rebuild and the comparison.

What the goldens cannot show: no domain-A dump has a support layer (SUPL), a painted facet or an
entity of kind 2, 3, 4 or 7 (contoured, sloped and oriented paths, sloped loops), and their step
warnings are few (one step's single warning, in 60 dumps); unit tests cover the entity kinds and
the step-state rebuild. The front-objects dump is read but not written again (its writer is
orca-cli's).

## Dependencies

Every library crate of phase O2 and every crate of phase O3 before it in the build order:
`orca-arrange`, `orca-config`, `orca-crt`, `orca-eigen`, `orca-exact`, `orca-fmt`, `orca-geom`,
`orca-json`, `orca-mesh`, `orca-model`, `orca-placeholder`, `orca-qhull`, `orca-stl`, `orca-tbb`,
`orca-xml`, `orca-zip`, `orca-index`, `orca-paths`, `orca-voronoi`, `orca-slice`. `orca-probe` for
its tests. Nothing from crates.io.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.
