# The sketch-orca protocol

How a program runs `sketch-orca`: its three commands, the job file it reads, and the result,
progress and settings files it writes, plus the manifest of a staged engine folder. A program that
runs the engine (a client, such as Sketch) talks to it only through these: it writes a job, starts
the engine as a separate process, reads the progress file while it runs, and reads the result when
it ends.

This folder is licensed under the MIT licence (`LICENSE` here): the specification, the JSON Schemas
in `schema/` and the examples in `examples/`, so that any program may implement a client of the
engine, or another engine. The engine itself, the rest of this repository, is licensed under the
GNU Affero General Public License, version 3 (`../LICENSE`).

| File | What it describes |
| --- | --- |
| `schema/job.schema.json` | the job, `sketch-orca-job/1` |
| `schema/result.schema.json` | the result, `sketch-orca-result/1` |
| `schema/progress.schema.json` | one line of the progress file |
| `schema/version.schema.json` | what `version` prints, and a result's `engine` |
| `schema/config-def.schema.json` | the settings catalogue, `sketch-orca-config-def/1` |
| `schema/engine-manifest.schema.json` | a staged engine's `manifest.json`, `sketch-orca-engine/1` |
| `examples/` | files that follow the schemas, one or more per format |
| `examples/invalid/` | files the schemas and the engine refuse |
| `examples/paths/absolute.json` | paths the engine does and does not read as absolute, on Windows and elsewhere |

The schemas are JSON Schema draft 2020-12. `test/protocol.test.ts` checks every example against its
schema and against the engine's own validators (`scripts/lib/job.ts`, `scripts/lib/stage.ts`),
which must agree.

## Conventions

- Every file is UTF-8 JSON. The progress file is newline-delimited JSON: one object per line.
- Every path in a job is absolute on the system the engine runs on (`/...` on Linux and macOS;
  `C:\...`, `C:/...` or a UNC share, `\\server\share\...`, on Windows), as Boost.Filesystem's
  `is_absolute` reads it. A Windows path from the root of the current drive (`\...` or `/...`), a
  drive without its root (`C:jobs`) and a server alone (`\\server`, and `//server` anywhere) are
  not absolute: the engine refuses them (`job-invalid`). `examples/paths/absolute.json` lists
  cases, for clients to check their own rule against. Paths in a manifest are relative to its
  folder, with forward slashes.
- An optional field of a job given as `null` is the same as one left out. Fields the formats do not
  name are ignored by the engine and should be ignored by clients, so fields can be added within a
  version.
- Each format names its version (`sketch-orca-job/1`). A change that a reader of the current
  version could not read gets a new version.
- The engine writes the result and the settings catalogue whole (to a temporary file it then
  renames), so a reader never sees half a file.

## Commands

```
sketch-orca version
sketch-orca config-def --out <file>
sketch-orca run --job <file> --out <file> [--progress <file>]
```

Options are `--name value` or `--name=value`, each given once and never empty. A command line the
engine does not take ends with exit code -2 (`invalid-params`) and its usage on stderr; for `run`,
when `--out` can be found, an `invalid-params` result is written there too. Nothing but a command's
own output goes to stdout. The engine logs to stderr; `SKETCH_ORCA_LOG_LEVEL` sets the level (0
fatal to 5 trace; 1, errors, by default).

- `version` prints one line of JSON (`schema/version.schema.json`): `engine` (`sketch-orca`),
  `engineVersion`, and the OrcaSlicer release it is built from, `orcaVersion`, `orcaTag` and
  `orcaCommit`.
- `config-def --out <file>` writes the settings catalogue (below).
- `run --job <file> --out <file> [--progress <file>]` does the job and writes its result, also when
  it fails; with `--progress`, it appends progress lines as it goes. It exits with the result's
  `exitCode`. The files of a run are separate files: the job, its model and presets, the result,
  the progress file, `output.gcode` and `output.bambu3mf`, with the files they are written through
  (the file's name plus `.tmp`; also `<result>.crash.tmp`, `<gcode>.tmp.postprocess` and
  `<bambu3mf>.rels.tmp`). Paths are compared as files: two existing paths to one file (a link, a
  short name) are one file, and on Windows letter case does not matter. The files a job names are
  every absolute path in its JSON, wherever it is (any setting's too), also when the job is not
  valid; all but `output.gcode` and `output.bambu3mf` count as its inputs. Outputs of the job that
  overlap each other or an input are `job-invalid`; a `--progress` or `--out` that overlaps another
  file is `invalid-params`, refused before the progress file is opened. A result that would replace
  the job or one of its inputs is not written at all: the engine says so on stderr and exits with
  -2. The same holds for a wrong command line: its result is written to `--out` unless that would
  replace a file the command line names or one its `--job` names.

## The job (`sketch-orca-job/1`)

```json
{
  "format": "sketch-orca-job/1",
  "command": "slice",
  "resources": "/opt/sketch-orca/resources",
  "workDir": "/jobs/1/work",
  "model": { "path": "/jobs/1/bracket.3mf" },
  "presets": { "machine": "/jobs/1/presets/machine.json", "process": "/jobs/1/presets/process.json", "filaments": ["/jobs/1/presets/filament_1.json"] },
  "overrides": { "sparse_infill_density": "15%" },
  "objects": [{ "index": 0, "overrides": {}, "layerHeights": { "mode": "fixed" }, "heightRanges": [{ "from": 0, "to": 5, "overrides": { "wall_loops": "4" } }] }],
  "customGcodes": [{ "printZ": 12.4, "type": "pause", "note": "Insert the 10 x 3 mm magnet" }],
  "arrange": false,
  "orient": false,
  "output": { "gcode": "/jobs/1/out/plate_1.gcode", "bambu3mf": "/jobs/1/out/plate_1.gcode.3mf", "stats": true }
}
```

| Field | Meaning |
| --- | --- |
| `command` | `slice` (G-code, and a project file when asked), `layers` (the layer list only) or `info` (the model and presets only, nothing sliced). |
| `resources` | The engine's resources folder: `engine.resources` of its manifest, made absolute. |
| `workDir` | A folder the engine may write scratch files to. |
| `model.path` | A `.3mf`, `.stl` or `.obj` file. Of a 3MF project the engine takes the objects, their parts and painting, and the filament each object and part prints with (`extruder`); the project's own settings for its objects and parts, height ranges and layer heights are not used. |
| `presets` | Flattened preset files (no `inherits` left to resolve): one machine, one process, and one filament for each filament the print uses, in extruder order. |
| `overrides` | Settings for the whole print, applied over the presets. |
| `objects` | Per object of the model, by its 0-based `index`: its own `overrides`, its `layerHeights` and its `heightRanges` (settings for a height range of the object). Objects not listed print with the plate's settings. |
| `customGcodes` | Pauses, colour and filament changes and custom G-code, each before a given layer. |
| `arrange` | Place the objects on the plate (default `true`). They are moved, never rotated; an object that does not fit is `arrange-failed`. With `false` the objects stay where the model puts them. |
| `orient` | Let the engine choose each object's orientation first (default `false`). |
| `output` | `gcode`: the G-code file to write (required for `slice`); `bambu3mf`: a `.gcode.3mf` project file to write as well, or `null`; `stats`: whether the result carries statistics (default `true`). |

- **Overrides** use OrcaSlicer's setting keys as OrcaSlicer writes them: a letter, then letters,
  digits and `_`, case-sensitive (`required_nozzle_HRC` is a key; `Layer_Height` is not). Values
  are text as OrcaSlicer writes them (`"15%"`, `"0.2"`, `"1"`); numbers and booleans are accepted
  and turned into that text (`true` is `"1"`). A key the engine's OrcaSlicer release does not have
  is `setting-invalid`. The keys of the preset files (`type`, `name`, `from`, `inherits`,
  `compatible_printers`) are `job-invalid` as overrides, in any letter case. An obsolete key that
  OrcaSlicer drops is `setting-invalid`; one it renames is applied under its new name for the
  values OrcaSlicer renames it for, and is `setting-invalid` with any other value. `config-def`
  lists which keys are which.
- **Layer heights** are `{ "mode": "fixed" }` (the process's layer height), `{ "mode": "adaptive",
  "quality": 0.5 }` (OrcaSlicer's adaptive layer height; 0 is the finest, 1 the coarsest, 0.5 by
  default), or `{ "mode": "profile", "profile": [z0, h0, z1, h1, ...] }`: pairs of a height on the
  object and the layer height from there on. Profile and height range heights are measured from the
  bottom of the object. A profile starts at `0` with the object's first layer height
  (`initial_layer_print_height`, or `layer_height` over a raft), rises, ends at the object's top
  (within 0.001 mm), and has no layer height above the smallest nozzle diameter; otherwise it is
  `layer-profile-invalid` (with the object's index). A layer height outside the printer's
  `min_layer_height`..`max_layer_height` is a warning, `layer-height-out-of-range`.
- **Custom G-codes** run before the layer whose print Z `printZ` falls on: the layer whose print Z
  is within half a layer of it (`custom-gcode-moved`, a warning, when it is not exactly on it).
  Pass a layer's exact print Z, from a `layers` job, to have it land where you mean it to.
  - `pause`: the printer's pause G-code (`machine_pause_gcode`; an empty one is
    `custom-gcode-invalid`). `note` is what the person at the printer does; it is kept for the
    client to show and in the project file, and is not sent to the printer.
  - `color` (with `color`, `#rrggbb`): with one filament, a pause to swap it by hand; with several,
    like `filament`, a switch to `extruder` from the first layer at or above `printZ` (every object
    must then print with one filament).
  - `filament` (with `extruder`, 1-based): switches to that filament. A change above half the first
    layer but within it starts the print on that filament (`custom-gcode-moved`).
  - `custom` (with `gcode`): that G-code.

  A `printZ` at or below half the first layer, or above the last layer, or two items on one layer
  (OrcaSlicer runs one per layer), is `custom-gcode-invalid`; any custom G-code with "print by
  object" is `unsupported`.

## The result (`sketch-orca-result/1`)

```json
{
  "format": "sketch-orca-result/1",
  "ok": true,
  "code": "ok",
  "message": "",
  "exitCode": 0,
  "command": "slice",
  "engine": { "engine": "sketch-orca", "engineVersion": "0.1.0", "orcaVersion": "2.4.2", "orcaCommit": "8500fcdccaa10b5099ac20d252af3a7c560046f1", "orcaTag": "v2.4.2" },
  "warnings": [{ "code": "custom-gcode-moved", "message": "...", "objectIndex": 0 }],
  "layers": [{ "objectIndex": 0, "ranges": [[0, 0.25], [0.25, 0.45]] }],
  "printZ": [0.25, 0.45],
  "outputs": { "gcode": "/jobs/1/out/plate_1.gcode", "bambu3mf": "/jobs/1/out/plate_1.gcode.3mf" },
  "stats": {
    "timeSeconds": 590.5,
    "layerSeconds": [210.1, 190.2],
    "filament": [{ "extruder": 1, "mm": 1315.6, "cm3": 3.16, "g": 3.93, "cost": 0.08 }],
    "features": { "Outer wall": { "seconds": 220.4, "mm": 512.3 } }
  }
}
```

- `ok` is true exactly when `code` is `ok` and `exitCode` is 0. `message` says what went wrong,
  naming the culprit where there is one; a failure about one object names it in `objectIndex`.
- `layers` gives each object's layers as `[bottom, top]` print Z, ascending; `printZ` the print Z
  of every layer of the plate, ascending. Print Z is the height of the top of a layer above the
  bed, not counting the printer's `z_offset` (G-code Z is print Z plus `z_offset`).
- `slice` returns everything above. `layers` returns `layers` and `printZ` and writes no G-code.
  `info` returns `info`: the model's objects (name, instances, triangles, and `min`, `max` and
  `size` in millimetres), the bed (`printableArea`, `printableHeight`, `excludeArea`, `bedType`)
  and the preset names, without slicing.
- `stats.features` is keyed by the feature's name as the G-code's type comments name it.
- A failed result has no `layers`, `printZ` or `outputs`.
- `warnings` do not fail the job. Their codes include `custom-gcode-moved`,
  `layer-height-out-of-range`, `filament-not-compatible`, `setting-substituted`,
  `preset-value-skipped`, `unknown-setting`, `validate-warning`, `gcode-warning` and
  `progress-unwritable`; a client shows the message and should accept codes it does not know.

## Errors and exit codes

The engine exits with the code the stock OrcaSlicer command line uses for the same meaning, and
gives it as the result's `exitCode`:

| Code | Exit code | Meaning |
| --- | --- | --- |
| `environment` | -1 | the resources folder is missing or unusable |
| `internal-error` | -1 | an error the engine did not expect, a crash inside OrcaSlicer's code included |
| `invalid-params` | -2 | the command line |
| `job-invalid` | -2 | the job file |
| `setting-invalid` | -2 | an override names no setting, or has a value the setting does not take |
| `layer-profile-invalid` | -2 | a layer height profile that does not fit the object |
| `custom-gcode-invalid` | -2 | a custom G-code that does not fit the layers or the filaments |
| `file-not-found` | -3 | a file the job names is missing |
| `preset-invalid` | -5 | a preset file that cannot be read |
| `model-unreadable` | -6 | a model that cannot be read, or an object with nothing to print |
| `not-fdm` | -7 | the printer is not a filament printer |
| `unsupported` | -8 | something the engine does not do |
| `output-failed` | -13 | an output that cannot be written |
| `out-of-memory` | -14 | out of memory |
| `process-not-compatible` | -17 | the process preset is not made for the printer |
| `config-invalid` | -18 | the merged settings fail OrcaSlicer's checks, or put the prime tower (`wipe_tower_x`, `wipe_tower_y`) off the printable area or on an excluded area |
| `arrange-failed` | -21 | an object, or the prime tower, does not fit on the plate |
| `orient-failed` | -22 | an object cannot be oriented |
| `empty-plate` | -50 | nothing left to slice |
| `validate-error` | -51 | the print fails OrcaSlicer's validation |
| `not-printable` | -52 | an object is partly outside the printable area (-50 when none is inside) |
| `filament-bed-mismatch` | -61 | a filament does not suit the bed type |
| `temperature-difference` | -62 | the filaments' temperatures are too far apart |
| `collision` | -63 | objects collide printing one by one (-64 printing layer by layer) |
| `slice-failed` | -100 | slicing failed |
| `path-conflicts` | -101 | the G-code paths conflict |
| `gcode-in-unprintable-area` | -102 | the G-code reaches outside the printable area, or the prime tower as sliced is off it or on an excluded area |

Other codes may appear in later versions; they are lower-case words joined by `-`. A crash inside
OrcaSlicer's code still writes an `internal-error` result, and the process ends with -1.

Process exit codes reach a client in different forms: Node on Windows reports -3 as 4294967293,
Bun on Windows as 253. Compare the low byte only when the exit code has a small code's form, from
-256 to 255 or from 0xFFFFFF00 up. A run that wrote no result and ended with -1 is
`internal-error`; so is a run ended by another Windows status (a stack overflow's 0xC00000FD),
which keeps its signed 32-bit value and is best named in hexadecimal.

## Progress

With `--progress <file>`, the engine appends one line per step:

```
{"stage":"slice","percent":14,"message":"slicing"}
```

The stages come in this order, as far as the job goes: `job`, `presets`, `model`, `orient`,
`arrange`, `slice`, `gcode` (OrcaSlicer's own lines during the export come under it), `3mf`,
`stats`, then `done` at 100 when the run is ok, or `failed` at the percent reached, with the
failure's message. The percent never goes down. A client reads the file from where it last stopped,
skips blank lines and a last line that is not complete yet, and accepts stages it does not know. A
progress file the engine cannot write is a warning, `progress-unwritable`, not a failure. The engine
opens the progress file once it has read the job and found the run's files apart, so the `job`
line comes then; a `--progress` that overlaps another file of the run is never written.

## The settings catalogue (`sketch-orca-config-def/1`)

`config-def` writes every setting OrcaSlicer defines for filament printers (`options`), each with
its `key`, `type` (OrcaSlicer's option type: `float`, `floats`, `int`, `percent`,
`floatOrPercent`, `bool`, `enum`, `string`, ...), the preset files it belongs to (`sections`:
`process`, `filament`, `machine`), OrcaSlicer's `label`, `fullLabel`, `tooltip`, `sidetext` and
`category`, its `mode` (`simple`, `advanced`, `develop`, `expert`), `default` (as OrcaSlicer writes
the value, or `null`), and where they apply `min`, `max`, `enumValues`, `enumLabels`, `guiType`,
`ratioOver` and `aliases`. The texts are OrcaSlicer's own, untranslated.

It also lists the keys OrcaSlicer drops or renames whenever it reads them, asked of OrcaSlicer's own
code: `obsolete` (sorted; none of them is an option), `renamed` (for each renamed one, the key
OrcaSlicer reads it as) and `stillDefined` (the obsolete keys OrcaSlicer still defines, left out of
`options`). `count` is the number of options.

## The engine folder (`sketch-orca-engine/1`)

A staged engine is a folder:

```
bin/            the executable (sketch-orca.exe on Windows) and the libraries it loads
resources/      what OrcaSlicer's library reads at run time: profiles/, info/, flush/
LICENSES/       the licences of the engine and of the code built into it, NOTICE.txt and AUTHORS.txt
manifest.json   sketch-orca-engine/1
```

The manifest (`schema/engine-manifest.schema.json`) names the executable (`engine.exe`), the
resources folder a job's `resources` names (`engine.resources`) and the folder of its libraries
(`engine.libraryPath`; on Linux and macOS put it on `LD_LIBRARY_PATH` or `DYLD_LIBRARY_PATH`), all
relative to the folder; the OrcaSlicer release (`orca`: repository, tag, version, commit, publish
date, licence); `platform` and `arch`; the patches with their SHA-256; whether the engine passed
its tests (`smoke`); the libraries shipped next to the engine (`runtime`) and those the system
provides (`system`); and every other file of the folder, sorted by path, with its size and
SHA-256 (`files`), so a client can check the folder before it runs the engine.

## Rules the schemas cannot state

The engine checks these, and so do the validators in `scripts/lib/`:

- Job: two objects with the same `index`; height ranges that overlap or are out of order, or whose
  `to` is not above `from`; a profile with an odd number of values, a height that is not above 0,
  or heights on the object that go down; two custom G-codes at one print Z. What depends on the
  model and presets (an object index the model does not have, a profile or custom G-code beyond the
  object's top, a key the release does not have) is checked when the job runs.
- Result: each object's layers and `printZ` ascend, and each layer's top is above its bottom.
- Manifest: `engine.exe` is one of `files`, and `files` lists the folder exactly.
