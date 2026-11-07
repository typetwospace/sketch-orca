# The parity corpus

`corpus.json` lists the jobs the Rust translation is held to the C++ engine on, and which families
of Orca's work each one exercises. `scripts/lib/parity/corpus.ts` defines it, and `bun scripts/orca.ts
parity corpus` writes it from the engine's checkout. The meshes stay in the work folder
(`<work>/parity/corpus/meshes/<mesh>/<file>`): `corpus.json` names each one's source and SHA-256.

## Cases

There are 166 cases, of four kinds. Each case runs in its own folder, as a smoke case does
(`same.ts` `runEngineCases`), so every output of every engine run is recorded the same way.

| Kind | Cases | What runs |
| --- | --- | --- |
| `smoke` | 18 | Each of the smoke run's cases (`smoke.ts` `SMOKE_CASES`) but `refusals`, with its own checks, as `smoke-<id>`. `refusals` runs every refused job in one case, across the families they reach, so the corpus leaves it out (`SMOKE_LEFT_OUT`): each refused job is a refusal case of its own. |
| `job` | 127 | A job written from the protocol's job schema. There are 84 mesh jobs, `mesh-<mesh>-<command>`: each of the 28 meshes below as an `info`, a `layers` and a `slice` job, on the generic preset set with no overrides. There are 7 Sketch-shaped jobs, `sketch-*`, below. There are 36 family jobs, `family-*`, below. |
| `example` | 7 | `protocol/examples/invalid/job-*.json`, its example paths moved into the case's folder (below), as `example-<name>`. |
| `refusal` | 14 | Each refused job of the smoke run (`smoke.ts` `REFUSALS`), one case each, as `refusal-<id>`. |

A job, example or refusal is written as it is: there are no checks of `job.ts`, because the engine
makes its own. It runs as `run --job job.json --out result.json --progress progress.ndjson` in the
case's folder. A case asks only that the engine finish and write a result: a refusal or a failed
slice is an output like any other.

### The meshes

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
the C++ engine's behaviour. They cannot certify as they stand; until `model-load` is ported every
job that loads a model is declined, so nothing waits on them. Before `model-load` is certified
they are named as engine faults in `scripts/lib/parity/corpus.ts`, taken out of their families'
cases with this reason, and the Rust program's defined answer for such a file (read it as the
other boxes are read, or refuse it as `model-unreadable`) is chosen with the `model-load` work.

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
  `filaments`, how many of the set's filaments it takes (all of them when it is not given), and
  `processChanges`, settings of the process preset the case changes (`templateSet`);
- `model`, the model's file name in the case's folder;
- `overrides`, `objects` and `customGcodes`;
- `arrange` and `orient`;
- whether it writes `gcode`, a `bambu3mf` and `stats`.

The record run writes the paths in each time:

- `resources` is the running engine's resources folder;
- `workDir` is `<case>/work`;
- `model.path` is `<case>/<model>`;
- the presets are the files of the set in `<cases>/presets/<set>/`, its first `filaments`, and
  with `processChanges` the case's own copy of the process, `<case>/process.json`
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

`FAMILIES` in `corpus.ts`, and `parity.json`, list 58 families:

- **The front end:** `version`, `config-def` and `intake` (the command line, the job, the presets
  stage and the model's file, and every refusal made there).
- **Preset conditions:** `preset-conditions`, a preset's compatibility condition
  (`compatible_printers_condition`, `compatible_prints_condition`) worked out for the job's
  printer and process.
- **Loading and placing the model:** `model-load`, `placement` and `info`.
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

Each case names the families it exercises. `parity corpus` works out a job's families from its
command and from its settings, reading each from the job's override, else from the preset set,
else from Orca's default in `probes/configdef.json` (`familiesFor`):

- **Walls:** the wall generator.
- **Infill:** the top, bottom and internal solid patterns, plus the sparse pattern when the density
  is above 0.
- **Other features:** supports and their type, a raft, ironing, fuzzy skin, and the prime tower
  (`enable_prime_tower`), which needs two filaments, smooth timelapse, or clumping detection with
  an exclusion area (`wrapping_exclude_area`) of more than two points, counted as Orca's reader
  splits the text at its commas (`settingPoints`), as Orca's `Print::has_wipe_tower` asks.
- **Contouring:** `zaa_enabled`.
- **Every slice:** the steps every slice runs.

A job also gets `preset-conditions` when Orca would work out a condition of its presets
(`presetFamilies`): a preset whose list (`compatible_printers`, `compatible_prints`) is empty and
whose condition is not, checked in Orca's order, the process for the printer first, then each
filament for the printer and, when that holds, for the process.

Smoke cases are named by hand (`SMOKE_FAMILIES`): `version` and `config-def` are those families,
`file-collisions` is `intake`'s, and `info`, `layers-only` and `sphere-adaptive` name their steps.
The rest, `crash-result` included (it crashes at `arrange`), count as a generic slice, with
`wipe-tower` for those that run a prime tower. These are named for the whole case, not worked out
for each of its runs, so `parity certify` holds a smoke case's runs to the goldens but not its
declines to these families (`DECLINE_HELD_KINDS`).

A refusal is held to the steps the C++ engine reaches before it refuses (`REFUSAL_FAMILIES`), as
it ends there: `flat-model` and `lone-triangle` to `model-load`, `too-big-for-the-bed` to
`model-load` and `placement`, `layers-above-the-nozzle` to those and `slice-mesh`,
`pause-below-first-layer` to every family of its slice but `gcode-export` and `gcode-processor`,
and `gcode-folder-unmakeable` to every family of its slice (its generic presets and the cube's
overrides). Every other refusal, and each example, is made while the job, its presets and its
model's file are read, and is `intake`'s: `intake` is certified on those runs, and only when
each gives the C++ engine's outputs (a declined run passes no family).

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
`parity record` keeps a family that was certified before as it is.
