# The parity corpus

`corpus.json` lists the jobs the Rust translation is held to the C++ engine on, and which families
of Orca's work each one exercises. `scripts/lib/parity/corpus.ts` defines it, and `bun scripts/orca.ts
parity corpus` writes it from the engine's checkout. The meshes stay in the work folder
(`<work>/parity/corpus/meshes/<mesh>/<file>`): `corpus.json` names each one's source and SHA-256.

## Cases

There are 305 cases, of four kinds. Each case runs in its own folder, as a smoke case does
(`same.ts` `runEngineCases`), so every output of every engine run is recorded the same way.

| Kind | Cases | What runs |
| --- | --- | --- |
| `smoke` | 18 | Each of the smoke run's cases (`smoke.ts` `SMOKE_CASES`) but `refusals`, with its own checks, as `smoke-<id>`. `refusals` runs every refused job in one case, across the families they reach, so the corpus leaves it out (`SMOKE_LEFT_OUT`): each refused job is a refusal case of its own. |
| `job` | 232 | A job written from the protocol's job schema. There are 84 mesh jobs, `mesh-<mesh>-<command>`: each of Orca's 28 meshes below as an `info`, a `layers` and a `slice` job, on the generic preset set with no overrides. There are 24 jobs of Orca's OBJ files read as OBJ, `mesh-obj-<mesh>-<command>`: all 21 as `info` jobs, and `extruder_idler_quads`, `V` and `A_upsidedown` as `layers` jobs too. There are 7 Sketch-shaped jobs, `sketch-*`, 36 family jobs, `family-*`, 29 jobs of the files the corpus makes, `model-*`, 15 placement jobs, `place-*`, 6 preset condition jobs, `preset-*`, 2 orient jobs, `orient-*`, and 29 process jobs, `process-*`, all below. |
| `example` | 7 | `protocol/examples/invalid/job-*.json`, its example paths moved into the case's folder (below), as `example-<name>`. |
| `refusal` | 48 | Each refused job of the smoke run (`smoke.ts` `REFUSALS`), one case each, as `refusal-<id>`: 14 refused from the job's reading to the G-code's writing; 14 of the presets stage and its folders, each changing one preset file, an override or a folder (`PRESET_REFUSE`), one of them a missing model after the stage's warnings; and 20 of the model's reading and of placement (`MODEL_REFUSE`, below). |

A job, example or refusal is written as it is: there are no checks of `job.ts`, because the engine
makes its own. It runs as `run --job job.json --out result.json --progress progress.ndjson` in the
case's folder. A case asks only that the engine finish and write a result: a refusal or a failed
slice is an output like any other.

### The meshes

There are 73 meshes: Orca's 28, its 21 OBJ files again as they are, and 24 files the corpus makes.

There are 28 meshes in Orca's `tests/data`, at the locked commit:

| Meshes | Source | In a case's folder |
| --- | --- | --- |
| 21 OBJ files | `20mm_cube`, `2x20x10`, `A`, `A_upsidedown`, `V`, `V_standing`, `bridge`, `cube_with_concave_hole`, `cube_with_concave_hole_enlarged`, `cube_with_concave_hole_enlarged_standing`, `cube_with_hole`, `extruder_idler`, `extruder_idler_quads`, `frog_legs`, `ipadstand`, `overhang`, `pyramid`, `simplification`, `sloping_hole`, `small_dorito`, `two_hollow_squares` (`.obj`) | `model.stl`, converted as below |
| `test_3mf/Prusa.stl` | a binary STL whose header holds `COLOR=` | `Prusa.stl`, as it is |
| `test_stl/ASCII/20mmbox-{CR,CRLF,LF,nonstandard}.stl` | ASCII STL with CR, CRLF, LF and nonstandard line ends | under its own name, as it is |
| `test_stl/Geräte/20mmbox-čřšřěá.stl` | a binary STL with a non-ASCII name | `20mmbox-čřšřěá.stl`, as it is |
| `test_3mf/Geräte/Büchse.3mf` | a 3MF with a non-ASCII name | `Büchse.3mf`, as it is |

A mesh's id is its name in lower case with `-` for `_`. The STL files get `prusa-stl` and
`20mmbox-cr`, `-crlf`, `-lf`, `-nonstandard` and `-unicode`. The 3MF gets `buchse-3mf`.

The 21 OBJ files also go into a case's folder as they are, under their own names (kind
`obj-file`, id `obj-<mesh>`), so Orca's OBJ reader reads them: quads, normals and all. Sketch never
hands the engine an OBJ, but a job may name one.

### The files the corpus makes

`MADE_FILES` in `corpus.ts` writes 24 files (kind `made`, source `corpus.ts MADE_FILES <id>`).
Each is the same bytes on every run, so `corpus.json` names its SHA-256 like any other:

| Mesh | What it is |
| --- | --- |
| `obj-texcoords` | the 20 mm cube as an OBJ with texture coordinates (`f v/vt`) |
| `obj-mtl` | the cube in two materials: `mtllib painted.mtl`, `usemtl` lines, and `painted.mtl` beside it (a companion, below); an engine fault, as the library's path is under 16 bytes (below) |
| `obj-mtl-long` | the same cube and library, named in 16 bytes or more: `painted-long.obj` with `mtllib painted-library.mtl` (19 bytes) and `painted-library.mtl` beside it; the C++ engine loads it |
| `obj-mtl-missing` | the cube naming a material library that is not there, which Orca logs and reads past |
| `obj-mixed` | quads, normals, texture coordinates, negative indices, a group and smoothing together |
| `3mf-two-objects` | the 20 mm cube placed twice and a block turned a quarter turn |
| `3mf-components-inch` | an object made of two components, one turned, in inches |
| `3mf-bambu` | a Bambu Studio project: objects of components in sub-models, `Metadata/model_settings.config` with names, extruders and the plate, a body on extruder 2 painted with supports, a seam, colours and fuzzy skin, a modifier of 80 % infill, and a second object not printable; with `3mf-bad-settings-xml`, the one painted model (`painting` in `corpus.json`) |
| `3mf-only-modifiers` | an object of modifiers only, no part to print |
| `3mf-one-off-bed` | two boxes, one of them partly off the bed |
| `3mf-not-printable` | two boxes, both marked not printable |
| `3mf-not-printable-off-bed` | the same two boxes, both marked not printable, at -100,-100 and 400,400, off the bed |
| `3mf-two-large` | two boxes that do not fit on the bed together |
| `3mf-not-a-zip`, `3mf-zip-without-model`, `3mf-truncated`, `3mf-bad-crc` | text that is no zip, a zip with no model, an archive cut in half, a stored model with one byte changed after its CRC-32 |
| `3mf-bad-model-xml`, `3mf-bad-rels-xml`, `3mf-bad-settings-xml` | broken XML in the model, the relationships and the Bambu settings |
| `3mf-vertex-out-of-range`, `3mf-no-build-items`, `3mf-empty` | a triangle past its vertices, a model with no build item, an empty file |
| `obj-empty` | an empty OBJ |

A 3MF is a zip from the corpus's own writer, `writeZip`: each entry deflated through node:zlib
(`deflateRawSync`, level 9) or stored, version 20, the fixed date 1980-01-01 00:00, flag 0x800 for
a name that is not ASCII, no extra fields, no ZIP64. `scripts/lib/zip.ts` reads it back, so the
tests check the writer against the reader. A made mesh can have companions, files written beside
it (`companions`); `corpus.json` names each companion's SHA-256 (`companionSha256`), and the record
run checks them as it checks the mesh.

### An engine fault: 20mmbox-CR.stl

In every pass of `parity record` (the oracle at 1, 2, 8 and 32 threads, the shipped engine five
times), `mesh-20mmbox-cr-info`, `-layers` and `-slice` end the same way: exit code 9 as the
harness records it, no result, nothing on stdout or stderr, and a progress file that stops at
`model` ("reading the model"). The C++ engine dies while it loads the model. The CRLF, LF and
nonstandard boxes load.

The likely cause, from reading the code (not proven by a debugger): admesh's ASCII reader
(`deps_src/admesh/stlinit.cpp`, `stl_read`) reads the solid's name with
`fscanf(fp, " solid %[^\n]", solid_name)` into `char solid_name[256]`. A file whose lines end with
a CR alone has no `\n`, so all 3200 bytes go into the 256-byte stack buffer. A stack overrun ends
an MSVC program with a fast fail (`STATUS_STACK_BUFFER_OVERRUN`, 0xC0000409, whose low byte is 9),
which no exception handler sees, so the engine's crash handler writes no `internal-error` result.

Decided (2026-11-06): these three cases are engine faults. What the C++ engine does there is
undefined behaviour, so the Rust translation does not reproduce it, and `parity certify` is never
to hold the Rust program to them. They stay in the corpus, so `record.json` shows any change in
the C++ engine's behaviour. They cannot certify as they stand: they are named as engine faults in
`scripts/lib/parity/corpus.ts` (`fault` in `corpus.json`) and held to no family, and `model-load`
was certified without them (2026-11-12). The Rust program's defined answer for such a file is
`model-unreadable` (below).

The cause, confirmed (o2-mesh, 2026-11-07): the cstdio probe's first run made that same
`fscanf(" solid %[^\n]")` call on text with CR line ends, with the static UCRT the engine links,
and died the same way, exit code 9 with nothing written. The probe now leaves the call out
wherever the line holds 256 bytes or more (CFIL call 8, `parity/SOP1.md`). The translation's
answer: `orca-mesh`'s `stl_open` refuses any ASCII file whose `solid` line is longer than 255
bytes with `StlError::OverlongSolidLine`, before it reads a facet, and leaves the mesh empty
(`crates/orca-mesh/tests/stl_open.rs`: a CR copy of a 20 mm cube is refused, its LF and CR LF
copies read, a 255-byte line reads and a 256-byte one is refused). The `model-load` work maps
that error to the job's answer, `model-unreadable`. The mesh and qhull probes, which read
OrcaSlicer's test STL files through the engine's own reader, leave this one out
(`parity/SOP1.md`, MCAS and QHUL).

Done (o2-info, 2026-11-10): the three cases are named in `ENGINE_FAULTS` (`corpus.ts`) with this
reason, so no family is certified or declined on them.

### An engine fault: an OBJ with its material library

`load_obj` (`Format/OBJ.cpp:56`) keeps `c_str()` of a temporary path string and reads the
library's path through that pointer after the string is gone: once for `mtlparse` (line 58) and
once for the log line (line 59 when the parse fails, line 65 when the library is missing). A case
runs in its own folder, so a library beside the OBJ is found by its bare name, and that name is
the path the pointer reads.

What the staged engine and the staged oracle give, two passes each, on the cube in two materials
(`model-obj-mtl-*`, `model-obj-mtl-long-*`, info and layers jobs):

- `painted.mtl`, 11 bytes (and a 15-byte name, in the review of 2026-11-10): `model-unreadable`,
  exit -6, "load mtl in obj: failed to parse", and stderr `[error] load_obj:load_mtl: failed to
  parse ` with an empty path.
- `painted-library.mtl`, 19 bytes (and names of 16 and 24 bytes, in that review): `ok`, one
  object, nothing on stderr.
- `missing.mtl`, not there (`model-obj-mtl-missing-*`): `ok`, one object, and stderr
  `[error] load_obj: failed to load mtl_path:<case folder>/missing.mtl`, written through the same
  pointer (line 65). That path is the case folder and the name, far over 16 bytes.

Every output was the same between the two passes, on both programs. The likely reading, not
proven with a debugger: a path under 16 bytes sits in the dead string's own buffer, which is empty
by then, so `mtlparse` opens "" and fails; at 16 bytes or more the freed block still holds the
path (`parity/HAZARDS.md`, the OBJ material library row).

Decided (2026-11-10, the orchestrator's ruling): what the staged C++ engine and the staged oracle
both give for an input, the same on two passes, is the reference the Rust program is held to,
whatever in the C++ produces it. A read through the freed path is not by itself an engine fault.
A case is one only when its answer is one the same input would not give with nothing changed but
the length of a path, or when its output moves between passes. So:

- `model-obj-mtl-info` and `-layers` are engine faults (`ENGINE_FAULTS`): the library's path is
  under 16 bytes, and the same OBJ with a longer name loads. Recorded, held to no family. The Rust
  translation (`orca-model`'s `obj.rs`) reads the library the OBJ names, as the code means to.
- `model-obj-mtl-long-info` and `-layers` are held to their families (model-load and info; every
  family of a domain-A layers job): the C++ engine reads the library there.
- `model-obj-mtl-missing-info` and `-layers` are held to theirs, their stderr line included.

If a later `parity record` shows the output of a held case here moving between passes, that case
becomes an engine fault then, named in `ENGINE_FAULTS` and in this section.

### model.stl as Sketch writes it

Sketch hands the engine its model as a binary STL, `model.stl`. An OBJ mesh of the corpus becomes
one the same way (`objToBinaryStl`):

- **The header:** 80 bytes, the ASCII bytes `Sketch` followed by zero bytes.
- **The triangle count:** a little-endian u32.
- **Each triangle:** 50 bytes.
  - The normal comes first: the unit vector of (b − a) × (c − a). It is computed in double
    precision from the corners as given and divided by its length, or it is (0, 0, 0) when the
    triangle has no area.
  - The three corners a, b and c follow.
  - The normal and the corners are each three little-endian float32, rounded from double.
  - Last is a u16 attribute of 0.

An OBJ's corners are its `v x y z` lines, read as doubles. A face's corners are the vertex numbers
before any `/`: 1-based, or negative to count back from the last vertex read. A face of n corners
becomes the fan (1, k, k+1) for k = 2..n−1, as Orca's OBJ reader splits a quad. Other lines (normals,
texture coordinates, groups) are left out.

### Jobs, made concrete

`corpus.json` holds each job as a template:

- `command`;
- `presets`, one of the smoke run's preset sets (`generic`, `generic2`, `bambu` or `h2d`), with
  `filaments`, how many of the set's filaments it takes (all of them when it is not given),
  `processChanges`, settings of the process preset the case changes, and `filamentChanges`,
  settings of each of its filament presets the case changes (`templateSet`);
- `model`, the model's file name in the case's folder;
- `overrides`, `objects` and `customGcodes`;
- `arrange` and `orient`;
- whether it writes `gcode`, a `bambu3mf` and `stats`.

The record run writes the paths in each time:

- `resources` is the running engine's resources folder;
- `workDir` is `<case>/work`;
- `model.path` is `<case>/<model>`, with a made mesh's companions copied beside it;
- the presets are the files of the set in `<cases>/presets/<set>/`, its first `filaments`, with
  `processChanges` the case's own copy of the process, `<case>/process.json`, and with
  `filamentChanges` its own copies of the filaments, `<case>/filament_<n>.json` from 1
  (`templateFiles`);
- the outputs are `<case>/out.gcode` and `<case>/out.gcode.3mf`. A job without G-code writes `""`.

### The Sketch-shaped jobs

These jobs are written from the protocol's job schema as Sketch writes them. They use the 20 mm
cube, which stands flat on the bed and is flat on top, so every layer up to the top prints. They
print at 0.2 mm layers from a 0.2 mm first layer, with `PLAIN_OVERRIDES`.

| Case | What it adds |
| --- | --- |
| `sketch-layers-fixed` | layers at fixed heights, no profile |
| `sketch-layers-profile` | layers from a boundary profile, `[0, 0.2, 5, 0.12, 12, 0.24, 20, 0.2]`, which ends at the top |
| `sketch-slice-pauses` | a slice with two pauses, at 2 and 4 mm |
| `sketch-slice-inserts` | a colour change inserted at 3 mm |
| `sketch-slice-height-ranges` | a height range from 2 to 6 mm with infill and walls of its own |
| `sketch-slice-custom-gcode` | custom G-code at 5 mm |
| `sketch-slice-timelapse-tower` | one filament on the Bambu X1 Carbon, a prime tower through smooth timelapse (`timelapse_type` 1, `enable_prime_tower` 1), written as a `.gcode.3mf` too |

### The family jobs

The presets leave some families off. A family job exercises one of them, switched on by its setting:

- each of the 26 sparse infill patterns on the 20 mm cube at 25 % (`family-infill-<pattern>`);
- the classic and the Arachne wall generators on the 20 mm cube;
- normal supports, tree supports and a 2-layer raft on the overhang;
- ironing, fuzzy skin on the outer walls, and Z contouring (`zaa_enabled`) on the 20 mm cube;
- clumping detection (`enable_wrapping_detection` 1) on the H2D set with one filament, whose
  printer has an exclusion area for it, which puts a prime tower on a single filament
  (`family-wipe-tower-clumping`);
- an `info` job whose process has no list of printers but a compatibility condition
  (`compatible_printers_condition` `nozzle_diameter[0]==0.4`), which Orca works out with its
  placeholder parser (`family-preset-conditions`).

### The jobs of the model's reading, placement, presets and orient

Phase O2 added the cases its families lacked. Each is on the generic set unless it says otherwise.

- **OBJ files as OBJ** (`mesh-obj-<mesh>-info`, and `-layers` for `extruder_idler_quads`, `V` and
  `A_upsidedown`): Orca's own OBJ reader on its own files.
- **The made files** (`model-<mesh>-info` and `-layers`): each OBJ and 3MF the corpus makes that
  loads, both ways (`3mf-bambu` on `generic2`, for its second extruder), and `obj-mtl` beside
  them, which the C++ engine fails to load (the engine fault above); the object of modifiers
  only as an `info` job, which checks no solid and answers, and as a `layers` job, which
  `check_solid_objects` refuses (`model-unreadable`, object 0); and each unreadable file as an
  `info` job (`model-unreadable`). `cli/run.cpp`'s refusal of a model that "has no objects"
  cannot be reached, because `Model::read_from_file` throws first; `refusal-stl-no-triangles`
  (`Loading of a model file failed.`) and `model-3mf-no-build-items-info` (the file "couldn't be
  read because it's empty") stand in for it.
- **Placement** (`place-*`, `layers` jobs, which reach the placement without slicing):

  | Case | What it does | How the C++ engine ends |
  | --- | --- | --- |
  | `place-several-objects` | `3mf-two-objects` arranged | done |
  | `place-arrange-off` | the same where its file puts them (`arrange` false) | done |
  | `place-arrange-off-partly-outside` | arrange off, one object over the bed's edge | `not-printable` -52, object 1 |
  | `place-every-object-not-printable` | arrange off, both objects marked not printable | `empty-plate` -50, in the slice stage |
  | `place-no-object-inside` | arrange off, both objects marked not printable and off the bed (`3mf-not-printable-off-bed`) | `empty-plate` -50, at the build volume check |
  | `place-bed-exclude-area` | the cube around an excluded area in the middle of the bed | done |
  | `place-round-bed` | two objects on a round bed, a 36-gon of radius 120 mm around 0,0 | done |
  | `place-tower-default-corner` | two filaments: the prime tower at its default corner | done |
  | `place-tower-off-excluded-area` | the same, moved off an excluded area over that corner | done |
  | `place-tower-set-by-job` | the same where the job sets it (`wipe_tower_x`, `wipe_tower_y`) | done |
  | `place-by-object` | two objects printed by object | done |
  | `place-temperature-difference` | PLA and PC on two objects | `temperature-difference` -62, while arranging |
  | `place-objects-do-not-fit` | `3mf-two-large` | `arrange-failed` -21, object 0 |
  | `place-object-settings` | the cube with settings of its own and two height ranges | done |
  | `place-custom-gcodes` | two filaments: a pause, a colour change, custom G-code and a filament change | done |

- **Preset conditions** (`preset-*`, `info` jobs of the cube): a filament whose compatibility with
  the process is a condition; a filament and a process whose condition is false
  (`nozzle_diameter[0]==0.6`: the warning `filament-not-compatible`, the refusal
  `process-not-compatible` -17); a condition the placeholder parser cannot parse, on the process
  and on a filament's condition for the process (Orca takes the preset either way); and one with
  a regular expression. Only the filament's `compatible_prints_condition` prints its parse error
  on stdout (`Preset.cpp:785`, a `printf`: "Preset::is_compatible_with_print - parsing error of
  compatible_prints_condition ...", the parser's message and the condition with a caret), so
  `preset-filament-condition-unparsable` holds the one stdout text the presets stage writes. The
  process's `compatible_printers_condition` gives no line at the default log level (`Preset.cpp:833`
  only logs a Boost.Log warning, which the engine does not print).
- **Orient** (`orient-info`, `orient-layers`): the cube with `orient` on. Each is held to `orient`
  alone (`withFamilies`): `sketch-orca-rs` declines an orient job where orientation would start,
  before any output, and a declined run counts against every family its case lists, so
  `model-load`, `placement` and `info` could never certify while these cases listed them.

A job that the C++ engine refuses before the slice says where (`stopsAt`, one of `STOP_ORDER`:
`preset-conditions`, `model-load`, `orient`, `placement`): its families are cut there
(`familiesUpTo`), as a refusal's are. `place-every-object-not-printable` has none: Orca's
`Print::apply` leaves the print empty after the `slice` line, before `print.process()`. A layers
run is answered whole, refusal and all, from `orca-o3-process-1` on, so it holds every family a
domain-A layers job does (below).

### The process jobs

Phase O3 ports what `print.process()` and the layers answer run in domain A (below). Its families
had cases on the generic presets with Orca's defaults, but `perimeters-classic`,
`infill-rectilinear` and `infill-concentric` had one slice job each and no layers job. The 29
process jobs, `process-*` (`processJobs`), are layers and slice jobs on the generic set, on meshes
the corpus has already, each with the settings of one feature:

| Case | Mesh | What it sets |
| --- | --- | --- |
| `process-walls-classic-layers`, `-slice` | `cube_with_hole`, `cube_with_concave_hole` | the classic wall generator |
| `process-walls-classic-thin-slice` | `two_hollow_squares` | the classic walls on thin walls |
| `process-walls-arachne-layers` | `two_hollow_squares` | Arachne set by name, 4 walls |
| `process-infill-rectilinear-layers`, `-slice` | the 20 mm cube, `V` | rectilinear sparse infill, 25 % and 40 % |
| `process-surfaces-rectilinear-slice` | `cube_with_hole` | rectilinear top, bottom and internal solid infill |
| `process-infill-concentric-layers`, `-slice` | the 20 mm cube, `cube_with_concave_hole` | concentric sparse infill, 25 % and 30 % |
| `process-surfaces-concentric-slice` | `cube_with_hole` | concentric top and bottom |
| `process-surfaces-swapped-slice` | the 20 mm cube | monotonic top, monotonic line bottom and internal solid |
| `process-elephant-foot-layers`, `-slice` | the 20 mm cube, `cube_with_hole` | elephant foot compensation, 0.2 mm over 3 layers and 0.15 mm |
| `process-polyholes-layers`, `-slice` | `extruder_idler`, `sloping_hole` | round holes as polyholes (not twisted on the slice) |
| `process-brim-outer-slice` | the 20 mm cube | an outer brim of 5 mm, no skirt |
| `process-brim-inner-layers` | `cube_with_hole` | an outer and inner brim of 3 mm |
| `process-skirt-slice`, `-layers` | `2x20x10`, the 20 mm cube | a skirt: 2 loops 4 mm out; 3 loops 2 layers tall |
| `process-several-objects-slice` | `3mf-two-objects` | its three instances of two objects arranged and sliced |
| `process-adaptive-layers`, `-slice` | `overhang`, `bridge` | adaptive layer heights, quality 0.5 and 0.3 |
| `process-profile-slice` | the 20 mm cube | a boundary profile, `[0, 0.2, 5, 0.12, 12, 0.24, 20, 0.2]` |
| `process-height-range-layers`, `-slice` | the 20 mm cube | a height range from 6 to 12 mm at 0.12 mm layers |
| `process-object-levels-layers` | the 20 mm cube | the object's own classic walls and 30 %, height ranges of rectilinear (4 to 8 mm) and concentric (12 to 16 mm) |
| `process-object-levels-slice` | `cube_with_hole` | the object's own classic walls, a height range of concentric (3 to 7 mm) |
| `process-layer-height-above-nozzle-layers` | the 20 mm cube | a layer height of 0.5 mm on the 0.4 mm nozzle |
| `process-fill-multiline-layers` | the 20 mm cube | `fill_multiline` 2 |

`process-layer-height-above-nozzle-layers` is refused (`PROCESS_REFUSED`): `Print::validate` gives
`validate-error`, "Layer height cannot exceed nozzle diameter.", before `print.process()`. As a
layers run is answered whole, it holds every family a domain-A layers job does, all past `intake`
(`crates/orca-cli/tests/intake.rs` holds only the refusals whose one family is `intake`).
`process-fill-multiline-layers` is outside domain A on purpose: no other case sets multiline
infill, so it is the one case of `fill-multiline`, and it holds `model-load`, `placement` and
`fill-multiline` alone (below). Every other process job is in domain A.

### The invalid examples, moved

The examples in `protocol/examples/invalid` use paths that are not paths on the machine running the
case: `/opt/sketch-orca/resources` and `/jobs/3/...`. In a case's folder, each becomes a real path:

- the resources folder becomes the running engine's;
- the machine, process and filament presets become the generic set's files;
- every other path under `/jobs/3/` becomes the same name in the case's folder;
- a model there is written as a 20 mm cube.

A path that is not under `/jobs/3/` stays as it is: `job-relative-model-path.json`'s `cube.stl` is
what that example is about. So each example is refused for its own rule, not for its paths.

## Families

`FAMILIES` in `corpus.ts` lists 66 families, and `parity.json` lists them once `parity record` has run on this corpus:

- **The front end:** `version`, `config-def` and `intake` (the command line, the job, the presets
  stage and the model's file, and every refusal made there).
- **Preset conditions:** `preset-conditions`, a preset's compatibility condition
  (`compatible_printers_condition`, `compatible_prints_condition`) worked out for the job's
  printer and process.
- **Loading and placing the model:** `model-load`, `orient` (orienting the objects, when the job
  asks), `placement` and `info`.
- **Slicing and walls:** `slice-mesh`, `perimeters-classic` and `perimeters-arachne`.
- **The 30 infill patterns** of Orca's `InfillPattern` (`infill-<key>`). `infill-tri-hexagon` is
  `ipStars`. `infill-supportbase` and `infill-concentricinternal` are the two patterns Orca uses
  only inside, for support bases and for narrow internal solid infill.
- **Supports and other features:** `supports`, `tree-supports`, `raft`, `ironing`, `fuzzy-skin`
  and `wipe-tower`.
- **G-code:** `gcode-export` and `gcode-processor`.
- **The rest, by Orca step** (`step-<step>`): estimating curled extrusions, preparing infill,
  contouring, simplifying paths and support paths, detecting overhangs for lift, simplifying walls
  and infill, skirt and brim, and the conflict check.
- **Phase O3's,** after all of those, so `parity.json` keeps its order; each names in `step` the
  step where it runs:
  - `tool-ordering` (`psWipeTower`): the tool ordering of a print by layer with no prime tower
    (`ToolOrdering`: each layer's filaments, and the custom G-code placed among them);
  - `support-check` (`posSupportMaterial`): with supports off, `generate_support_material`'s
    check whether an object needs supports (`is_support_necessary` and its warning);
  - `multi-filament` (`psWipeTower`), `painted-facets` (`posSlice`), `print-by-object`
    (`psSkirtBrim`) and `fill-multiline` (`posInfill`): marks of a job outside domain A (below);
  - `layers` (`layers`): the layers answer, the result's layer list and `printZ`, which
    `cli/run.cpp` writes once `print.process()` has returned (lines 1855-1859).

Each case names the families it exercises. `parity corpus` works out a job's families from its
command, its model and its settings, reading each from the job's override, else from the preset
set, else from Orca's default in `probes/configdef.json` (`familiesFor`):

- **Every job:** `model-load`, then `orient` when the job orients its objects. The corpus holds
  such a job to `orient` alone, as `sketch-orca-rs` declines it before any output (above);
  `familiesFor` still names the others, as `decline.rs` does.
- **The command:** `info` for an info job, which is answered before the objects are placed
  (`cli/run.cpp` `run`), so it has no `placement` and reads no other setting; `placement` and
  `slice-mesh` for a `layers` or `slice` job, which then runs `print.process()`.

A layers or slice job's settings are read at every level: the job's own (overrides, presets,
defaults), then each object's overrides over them, then each height range's over its object's
(`objectLookups`). Each level adds what it turns on (`settingFamilies`); the job's own level always
counts, even where every object overrides a setting:

- **Walls:** the wall generator: `perimeters-classic` for `classic`, else `perimeters-arachne`.
- **Infill:** the top, bottom and internal solid patterns, plus the sparse pattern when the density
  is above 0 (`infill-<pattern>`).
- **Supports:** as `PrintObject::has_support` has them, `enable_support` 1 or
  `enforce_support_layers` above 0: `tree-supports` for a `tree*` type, else `supports`, with
  `step-simplify-support-path` and `infill-supportbase`. With supports off, `support-check`.
- **Other features:** a raft (`raft_layers` above 0), ironing (`ironing_type` other than
  `no ironing`), fuzzy skin (other than `none` or `disabled_fuzzy`), and the prime tower
  (`enable_prime_tower` 1), which needs two filaments, smooth timelapse (`timelapse_type` 1), or
  clumping detection with an exclusion area (`wrapping_exclude_area`) of more than two points,
  counted as Orca's reader splits the text at its commas (`settingPoints`), as Orca's
  `Print::has_wipe_tower` asks.
- **Contouring:** `zaa_enabled` 1 gives `step-contouring`.
- **Printing by object:** `print_sequence` `by object` gives `print-by-object`.
- **Multiline infill:** `fill_multiline` above 1 where the sparse density is above 0 gives
  `fill-multiline` (`Fill.cpp` applies it to sparse infill alone).

Then, once for the job:

- **Filaments:** more than one filament in the job (its preset set's, or its first `filaments`)
  gives `multi-filament`.
- **The model:** a model with painting gives `painted-facets`. `parity corpus` reads each model's
  file once (`modelPainting`) and notes on its mesh entry the painting it holds (`painting`): a 3MF
  whose model files give one of the triangle attributes Orca's 3MF reader takes as painting
  (`paint_supports`, `paint_seam`, `paint_color`, `paint_fuzzy_skin`, `Format/bbs_3mf.cpp`) a value
  that is not empty. An STL never paints, nor does an OBJ: its colours never paint on the command
  line. A 3MF's own settings are not read: `cli/run.cpp` `load_model` keeps only each object's and
  part's extruder, and drops the project's height ranges and layer heights. Only `3mf-bambu` and
  `3mf-bad-settings-xml` are painted; `Büchse.3mf` is not.
- **Every process run:** `infill-concentricinternal` (`Fill.cpp` turns narrow internal solid infill
  into it), and the steps every process run takes: the eight `step-*` families other than
  `step-contouring` and `step-simplify-support-path`, and `tool-ordering`.
- **The end:** `layers` for a layers job; `gcode-export` and `gcode-processor` for a slice job,
  never `layers`.

A job also gets `preset-conditions` when Orca would work out a condition of its presets
(`presetFamilies`): a preset whose list (`compatible_printers`, `compatible_prints`) is empty and
whose condition is not, checked in Orca's order, the process for the printer first, then each
filament for the printer and, when that holds, for the process. `sketch-orca-rs` finds the same
(`decline::preset_families`) and works such a condition out in its presets stage
(`orca-placeholder`, 2026-11-07); its `front-objects` dump, once it writes one, is what passes
the family (below).

Smoke cases are named by hand (`SMOKE_FAMILIES`): `version` and `config-def` are those families,
`file-collisions` is `intake`'s, and `info` is `model-load` and `info`. `layers-only` and
`sphere-adaptive` run domain-A layers jobs alone, so they hold what `familiesFor` gives a layers
job on the generic presets (`PLAIN_OVERRIDES`), `layers` among them. The three that run a prime
tower (`SMOKE_TOWER_CASES`: `filaments`, `flush-volumes`, `prime-tower`) are outside domain A and
hold `model-load`, `placement` and `wipe-tower` (`SMOKE_TOWER_FAMILIES`). The rest, `crash-result`
included (it crashes on purpose at the slice stage, `SKETCH_ORCA_TEST_CRASH`), hold what a generic
slice does, never `layers`. These are named for the whole case, not worked out for each of its
runs, so `parity certify` holds a smoke case's runs to the goldens but not its declines to these
families (`DECLINE_HELD_KINDS`).

A refusal is held to the steps the C++ engine reaches before it refuses (`REFUSAL_FAMILIES`), as
it ends there: `flat-model` and `lone-triangle` to `model-load`, and so the refusals of the
model's reading (an empty file, a binary STL of no triangles or cut short, an ASCII STL with a
vertex that is not a number, an OBJ face past its vertices, and an object the model does not
have); `too-big-for-the-bed` to `model-load` and `placement`, and so the refusals of placement (an
object's or a height range's setting it does not take, a zero layer height, a filament past the
job's, custom G-code placement refuses or prints by object, a prime tower set past the bed or too
wide to fit, and a cube that arrange off leaves off the bed, `not-printable` with -50 as no object
is inside); `layers-above-the-nozzle` to those and `slice-mesh`, as `apply_layer_heights` refuses
it before `print.process()`; `pause-below-first-layer` to every family of its slice but
`gcode-export` and `gcode-processor`, and `gcode-folder-unmakeable` to every family of its slice
(its generic presets and the cube's overrides): both are refused once `slice_print` has returned,
so they hold every family `print.process()` runs, phase O3's among them, and never `layers`.
Every other refusal, and each example, is made while the job, its presets and its
model's file are read, and is `intake`'s: `intake` is certified on those runs, and only when
each gives the C++ engine's outputs (a declined run passes no family). `crates/orca-cli/tests/intake.rs`
holds the Rust program to each of those goldens as well, and fails when such a refusal has no
test there. The presets stage's `unknown-setting` warning has no case: Orca drops a key it does
not have before the value is set, so the C++ program never gives it (`cli/presets.cpp`).

### Phase O3: domain A's cut and the slice rule

Phase O3 ports `print.process()` and the layers answer for domain A: one filament, printed by
layer, no prime tower, supports, raft, ironing, fuzzy skin, Z contouring or painted facets, the
fill patterns `monotonic`, `monotonicline`, `rectilinear`, `crosshatch`, `concentric` and
`concentricinternal` (`DOMAIN_A_PATTERNS`), one line of sparse infill, the classic or the Arachne
walls, with the elephant foot and polyholes carried. Its families are `O3_FAMILIES`: `slice-mesh`,
the two wall families, those six patterns, the `step-*` families every process run takes,
`tool-ordering`, `support-check` and `layers`.

`certify` passes a family only when every run of every case of it passed, and a declined run only
through a boundary whose families hold it. So each case holds only the families its O3 run can
pass (`withFamilies`):

- **A domain-A layers job** is answered whole: every family `print.process()` runs, and `layers`.
- **A domain-A slice job** declines after `print.process()`, past the end-of-process boundary
  (patch 0007's `print-psConflictCheck` dump), before `cli/run.cpp` writes the layer list and
  `printZ`: every family `print.process()` runs and the G-code's, never `layers` (the slice rule).
  The phase that answers slice jobs gives them `layers`.
- **A job outside domain A** reaches a marker (`MARKER_FAMILIES`). The program declines it by name
  once its objects are placed, before `print.process()`, past the front-objects boundary. So it
  holds only its markers and the front-objects families it lists (`preset-conditions`,
  `model-load`, `placement`: `FRONT_OBJECTS_FAMILIES`), never an O3 family (domain A's cut,
  `domainACut`). A stop before the slice (`stopsAt`) and orient come first.
- **A run the C++ refuses before `slice_print` returns** is answered with that refusal; a layers
  run is answered whole in any case.

The markers, each told from the job and its model as `corpus.ts` tells it, so `decline.rs` can
mirror `familiesFor`:

| Marker | When |
| --- | --- |
| `infill-<pattern>` | a pattern outside domain A on the top, bottom or internal solid surface, or the sparse pattern when the density is above 0, at any level (job, object, height range) |
| `supports`, `tree-supports`, `step-simplify-support-path`, `infill-supportbase` | `enable_support` 1 or `enforce_support_layers` above 0, at any level |
| `raft`, `ironing`, `fuzzy-skin`, `step-contouring` | `raft_layers` above 0, `ironing_type` not `no ironing`, `fuzzy_skin` not `none` or `disabled_fuzzy`, `zaa_enabled` 1, at any level |
| `wipe-tower` | `enable_prime_tower` 1 with two filaments or more, `timelapse_type` 1, or clumping detection with more than two points of exclusion area, at any level |
| `multi-filament` | more than one filament in the job |
| `painted-facets` | the model's file holds painting (a 3MF's `paint_*` triangle attributes with a value; never an STL or OBJ) |
| `print-by-object` | `print_sequence` `by object`, at any level |
| `fill-multiline` | `fill_multiline` above 1 where the sparse density is above 0, at any level |

There are 42 cases outside domain A: the 41 of the corpus before phase O3 (the three prime tower
smoke cases and `sketch-slice-timelapse-tower`; the 23 sparse pattern family jobs outside domain A;
the supports, tree supports, raft, ironing, fuzzy skin, contouring and clumping family jobs;
`model-3mf-bambu-layers`, two filaments and painted; the three prime tower placement jobs and
`place-custom-gcodes`, two filaments each; `place-by-object`; and `place-object-settings`, whose
height range is gyroid), and `process-fill-multiline-layers`. Each O3 family has a domain-A layers
job, and each but `layers` a domain-A slice job; the process jobs (above) give `perimeters-classic`,
`infill-rectilinear` and `infill-concentric` their layers jobs and more slices.

## Recording

```
bun scripts/orca.ts parity corpus                    # the meshes, and corpus.json
bun scripts/orca.ts parity record [--only <regex>]   # the passes, the classes, the goldens
bun scripts/orca.ts parity envelope                  # the shipped engine's own variance
bun scripts/orca.ts parity goldens --check <folder>  # the H-sites
```

`parity record` runs every case in one folder, `<work>/parity/runs/record/cases`. All the passes
use it, so the paths in the jobs and the outputs are the same bytes. It runs these passes:

- **The oracle,** with `SKETCH_ORCA_ORACLE_TIME` and `SKETCH_ORCA_ORACLE_SEED` pinned (as
  `parity neutral` pins them), at `SKETCH_ORCA_ORACLE_THREADS` 1, 2, 8 and the machine's thread
  count, each once (`oracle-p<P>`).
- **The engine staged from this checkout,** five times, as it ships (`engine-1` to `engine-5`).

Each run is kept as `parity same` keeps it (`<work>/parity/runs/record/<pass>/<case>/<n>/`), with
the run's stdout, stderr and `files/`. `pass.json` beside it says what made the pass, so a pass
whose program, knobs and corpus have not changed is kept, not run again. `--only` runs the matching
cases into `runs/record-only` and writes nothing else.

It then writes these:

- `<work>/parity/record.json`, every output with its class;
- `<work>/parity/goldens/`, the oracle's pass at one thread, kept with `goldens.json`;
- the front end's goldens in `goldens/front/` here;
- `parity.json`.

### Classes

Each output of each engine run gets a class (`classes.ts`). An output is the exit code, stdout,
stderr, and each file the run made, changed or removed. The class is the first of these that holds:

| Class | When |
| --- | --- |
| `H` | It differs between two work folders (`parity goldens --check`), even with each folder's own paths written as below. |
| `R` | The engine differs between its own five runs where no mask applies. |
| `T` | The oracle differs between thread counts. With the clock and seed pinned, only the thread count moves. |
| `N` | The oracle differs from the engine where no mask applies, while neither moves by itself. The oracle is then not neutral there. |
| `E` | Only masked fields differ (`masks.json`): between the engine's runs, or between the oracle and the engine. |
| `B` | The bytes are the same everywhere. |

`flags` lists every class that holds. A family's classes are those of the outputs of the cases
that exercise it.

### The envelope

`parity envelope` reads, for each `R` output, all five of the engine's runs (`envelope.ts`):

- **A text output:** which lines vary. Where the runs' lines have the same text around their
  numbers, each number's least and greatest value and their spread. Any other line is kept as text
  that varies.
- **A binary output:** its sizes, and how many bytes differ from the first run's.

It writes `<work>/parity/envelope/envelope.json`.

### Two folders

`parity goldens --check <folder>` checks whether the outputs depend on the folder they were made in:

- It copies the staged oracle and the corpus into a second work folder (hard links where the volume
  allows).
- It runs the oracle there as the goldens were run: time and seed pinned, one thread.
- It compares every output with the goldens, with each folder's paths written as `<case>`,
  `<cases>`, `<resources>` and `<work>`.

An output that still differs is an H-site. It goes in `parity.json` (`hSites`), where it waits for
an oracle H-patch, and its class in `record.json` becomes `H`. A zip (a `.3mf`) is compared as its
bytes, so a path inside one makes an H-site too. The command exits 0 when it ran: an H-site is a
finding.

## The front end's goldens

`goldens/front/` holds what the C++ engine's front end writes, from the kept pass:

- `version.json` is `version`'s stdout.
- `config-def.json` is what `config-def` writes.
- `intake/<case>.result.json` is the `result.json` of each example and refusal case.

In a result, paths are written as `<case>`, `<cases>`, `<resources>` and `<work>`: the case's
folder, the cases' folder, the oracle's resources and the work folder. They are matched as written,
with forward slashes, and JSON-escaped, so a result's bytes do not name the machine that made them.

## parity.json

`parity.json` has the format `sketch-orca-parity/1`. It holds:

- `corpus`: the corpus file and its SHA-256;
- `threads` and `engineRuns`: what the record ran;
- `families`: each family, in `FAMILIES` order, with its `step`, the `cases` that exercise it, the
  `classes` of their outputs, and `certified`;
- `hSites`.

Every family starts as `certified: false`. Certifying a family is later work (`parity certify`);
`parity record` keeps a family that was certified before as it is. `parity certify` certifies a
family when every run of every case of it passes: its outputs the goldens', and the dumps the
goldens keep (`front-objects-<n>.sop1`, the model once the objects are placed, with the front
end's config from oracle patch 0006 on: `parity/SOP1.md`) the same. A run the
Rust program declines passes no family, with one exception: a right decline that wrote every dump
of a boundary as the C++ run did, its progress lines the C++ run's first lines, passes the families
wholly before that boundary (`front-objects`: `preset-conditions`, `model-load` and `placement`).
So a phase that ports the steps up to a boundary certifies their families on every case that goes
past it, slices included, while the rest still declines. A smoke case whose later run is made from
an earlier run's outputs (`pocket-pause`, `imported-settings`) goes on, after a run the program
declines, from the golden C++ run's outputs (`certify.ts` `goldenCarry`), so its later runs are
made with the C++ program's jobs and inputs and judged like any other.
