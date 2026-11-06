# The parity harness

The parity harness holds the Rust translation in `crates/` to the C++ engine. Every output the
Rust program writes must be byte for byte the one the C++ engine writes for the same job, or a
difference the harness can name and account for. This folder holds the harness's committed data.
Its code is in `scripts/lib/parity/`, and its large data lives in the work folder (below).

Run the harness through the pipeline:

```
bun scripts/orca.ts parity <command> [options]    # or: bun run engine parity <command>
bun scripts/orca.ts parity --help                 # the commands there are, each with its usage
```

Each command is one module, `scripts/lib/parity/cmd-<name>.ts`, found by name when it runs
(`dispatch.ts`). A name is a lower-case letter, then letters, digits and `-`. A module exports:

- `usage`: one line;
- `locked`: whether it writes in the work folder. A locked command holds `<work>/run.lock.d` while
  it runs, as `build`, `smoke` and `stage` do, so no two runs write there at once;
- `run`: the command itself, which returns its exit code.

A new command is a new module: nothing in `orca.ts` lists them. Each command logs to
`<work>/logs/parity-<name>.log`. Exit codes are the pipeline's: 0 done, 3 failed, 64 a command line
it does not take; a gate that ran and failed exits 1 (`parity neutral`, `parity probe --check`).

## Commands

| Command | What it does |
| --- | --- |
| `reference` | Downloads the published sketch-orca 0.1.0 engine from this repository's GitHub release v0.1.0: `sketch-orca-0.1.0-orca-2.4.2-win32-x64.zip` (40,813,895 bytes) and `SHA256SUMS`, never the 611 MB source zip. It checks the zip's size and SHA-256 while it streams (`github.ts` `downloadReleaseAsset`). The SHA-256 must be the one `SHA256SUMS` lists and the one pinned in `reference.ts`. It then unpacks the zip to `<work>/parity/reference/0.1.0` and checks every file against the engine's manifest. A second run checks what is there and downloads nothing. |
| `same` | Runs the reference and the engine staged from this checkout (`<work>/dist/engine`, after `build`, `smoke` and `stage`) on every smoke case's jobs (`smoke.ts` `SMOKE_CASES`, 19 cases), one engine after the other, in one folder, so the jobs and the paths in them are the same bytes. Every output of every engine run is compared: exit code, stdout, stderr, and each file the run made, changed or removed. Each is marked identical, masked (identical after `masks.json`) or different. The command writes `<work>/parity/same.json` and exits 0 whatever it finds: a difference is a finding, not a failure. |
| `oracle` | Builds the oracle, the engine with the knobs of `oracle/patches` (`oracle/README.md`), and stages it in `<work>/oracle/dist`. It refuses to start with less than 40 GB free on the work folder's drive or in a work folder longer than `WINDOWS_WORK_PATH_MAX`, builds on the engine's dependency build (`<work>/deps`, shared, never rebuilt) with the engine's toolchain and `/MT`, writes the linker map beside the program, and stages `bin/sketch-orca-oracle.exe`, its map, the libraries it loads, the engine's resources and `oracle.json` (`oracle.ts`, `BUILDING.md`). The oracle never ships. |
| `neutral` | The neutrality gate (`neutral.ts`), on every smoke case's jobs, run as `same` runs them: (a) with no knob set, the oracle's outputs equal those of `<work>/dist/engine` after `masks.json`; (b) with `SKETCH_ORCA_ORACLE_TIME` and `SKETCH_ORCA_ORACLE_SEED` set, two oracle runs are byte for byte the same, with no mask; (c) with `SKETCH_ORCA_ORACLE_THREADS=1` and `=8` (time and seed pinned, each run dumping SOP1 files, `SOP1.md`), every run finishes, and each output or dump that differs is recorded as a T candidate, with what the dumps' RUN records say TBB reported. It writes `<work>/parity/neutral.json` and exits 1 when (a) or (b) fails, 0 when both pass. It needs `parity oracle`, and `build`, `smoke` and `stage` of this checkout. |
| `probe [--check]` | Builds the probe program (`oracle/probe/`, `probe.ts`) against the oracle's build tree, with the compile line of the engine's `cli/config_def.cpp` and the link line of `sketch-orca.exe` read from the oracle's `build.ninja`, with CMake, Ninja and `cl` into `<work>/oracle/probe-build` (no Fortran check; no `.vfproj` or `CompilerIdFortran` may appear there), and runs it hidden. It records what the engine's foundations do: the static UCRT's maths functions (each one the oracle's linker map lists, with FMA3 on and off), MSVC's casts, `printf`, `to_chars`, iostream output and parsers, the STL's sorts, heaps, searches, hashes, unordered containers and random numbers, oneTBB's split trees for P from 1 to 64, and Orca's ConfigDef (`SOP1.md`, "Probe files"). The full set goes to `<work>/parity/probes`, the committed subset to `probes/` (under 8 MB). `--check` runs it again into `<work>/parity/runs/probe-check` and exits 1 unless every file of both sets is the same bytes. It needs `parity oracle`. |

## Every engine runs as a client runs it

Every run gets the environment a client that downloaded the staged engine gives it, as Sketch's
slicer does, and nothing else from the harness's shell (`engineEnv.ts`). On Windows that is
`SystemRoot`, `windir`, a `PATH` of the engine's library folder, `System32` and the Windows
folder, and `TEMP`, `TMP`, `LOCALAPPDATA`, `APPDATA` and `USERPROFILE`.

There is no `USER` or `USERNAME`, so Orca's `[user]` placeholder reads `unknown`, and no `SLIC3R_*`
variable. The harness refuses to start an engine in an environment that has either. Engines run
hidden, with stdin closed, and are ended when the smoke timeout runs out.

## Masks

`masks.json` lists what may differ between two runs of the same job: only the clock and the
entropy that reach the outputs, and where in a program's own code a crash happened (any two builds
place their code differently), each mask with the source line that writes it and why.

| Mask | Kind | What it covers |
| --- | --- | --- |
| `gcode-generated-on` | text | The `; generated by ... on <date> at <time>` header line (`GCode.cpp:2576`), in `.gcode` files and in a `.gcode.3mf`'s plate G-code. The generator's name and version stay compared. |
| `3mf-metadata-dates` | text | A 3MF model's `CreationDate` and `ModificationDate` metadata (`bbs_3mf.cpp:6834-6836`). |
| `3mf-gcode-md5` | md5-of | A plate G-code's `.md5` entry. It is masked only when each archive's MD5 is that of its own G-code, which is then compared on its own. |
| `zip-entry-times` | zip-times | The DOS times of a `.3mf`'s entries, in the local headers and the central directory. |
| `crash-location` | text | In a crash result (`result.json`), the program's file name and the offset in it where the crash happened (`cli/common.cpp:581`). The exception code and the stage stay compared. |

A mask applies only to the files its `files` globs name. A glob names an output by its file name
(`*.gcode`), or an archive's entry as `<archive>!<entry>`. `*` never matches across `/` or `!`.
An archive is masked only when its entry names, their order, the compression methods and the
central directory records are the same, and each entry is identical or masked. Text is compared
byte for byte. Any other difference is `different`, with the first line or entry where the outputs
part (`masks.ts`).

Clock and entropy that the masks do not cover are differences: `PlaceholderParser`'s timestamp
variables, the clock-seeded `mt19937` (`GCode.cpp:2705-2707`), and `random()` in a vendor's
templates. The oracle pins them (`SKETCH_ORCA_ORACLE_TIME`, `SKETCH_ORCA_ORACLE_SEED`,
`oracle/README.md`): with both set, two oracle runs need no mask at all (`parity neutral` (b)).

## The work folder

The harness writes only in the work folder (`%LOCALAPPDATA%\SketchDev\orca-engine`, or
`SKETCH_ORCA_HOME`). `scripts/lib/paths.ts` `parityPaths` names every folder:

```
parity/reference/<version>/   a published engine, unpacked and checked (reference)
parity/reference/<version>.json   what it was unpacked from: tag, asset, size, SHA-256, address
parity/reference/downloads/<version>/   the zip and SHA256SUMS as downloaded
parity/corpus/                the jobs and meshes the families are certified on
parity/goldens/               the C++ engine's outputs of the corpus
parity/runs/                  one folder per comparison run; same/ holds parity same's cases
                              (cases/), and each engine's recorded runs (reference/, head/:
                              <case>/<n>/ with stdout, stderr and files/); neutral/ holds parity
                              neutral's cases, its recorded runs (head/, oracle/, pinned-a/,
                              pinned-b/, threads-1/, threads-8/) and the dumps of the thread runs
                              (dumps-t1/, dumps-t8/: <case>/<n>/*.sop1)
parity/probes/                the probe program's full set (parity probe): crt.sop1, fmt.sop1,
                              stl.sop1, tbb.sop1, configdef.json
parity/runs/probe-check/      parity probe --check's second run (committed/, full/)
parity/envelope/              the shipped engine's own run-to-run variance
parity/same.json              parity same's report
parity/neutral.json           parity neutral's report
oracle/src/                   the oracle's checkout (a separate C++ build that never ships)
oracle/build/                 its build; the linker map is build/sketch-orca/sketch-orca.map
oracle/state/                 what the checkout and the build were made from
oracle/dist/                  its staged copy: bin/sketch-orca-oracle.exe, its map, the libraries
                              it loads, resources/ and oracle.json
oracle/probe-build/           the probe program's build: compile.rsp and link.rsp (the engine's
                              lines), crt-symbols.txt (the map's maths functions) and
                              sketch-orca-probe.exe
```

On Windows each folder counts against the work folder's length limit (`WINDOWS_WORK_PATH_MAX`).
Its place below the work folder, plus room for files 150 characters deeper
(`PARITY_FOLDER_DEPTH`), must fit in the 183 characters `build` keeps free below the work folder.
So any work folder `build` takes is one the harness can work in, and a command stops before
writing if one does not fit.

Only small data is committed here: `masks.json`, `SOP1.md` (the format of the oracle's dumps and
of the probe files), `HAZARDS.md` (every place the engine's bytes hang on more than its inputs,
with its source line, its class and what holds the translation to it), `oracle/` (the oracle's
patches and their README, and `oracle/probe/`, the probe program, with `oracle/probe/recorded/`,
the programs whose output the crates' own tests hold), `probes/` (the committed probe subset,
about 6.4 MB), and later `parity.json`, `corpus.json` and the front-end goldens. Meshes, slice
goldens and the full probe set stay in the work folder.

## Tests

`bun run test` covers the harness hermetically: fakes and fixtures only, never the work folder or
the network. The tests are `test/parity-dispatch.test.ts`, `test/parity-reference.test.ts` (a fake
GitHub and fetch), `test/parity-same.test.ts`, `test/parity-oracle.test.ts` (the oracle's patches
and knobs, the refusals of `parity oracle`, the SOP1 reader and what `parity neutral` makes of runs
and dumps), `test/parity-probe.test.ts` (the map's maths functions, the compile and link lines
read from a build.ninja, the probe's configure line, a fake probe run, and the committed probe
files), `test/github.test.ts` and `test/paths.test.ts`.
