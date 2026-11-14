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
it does not take; a gate that ran and failed exits 1 (`parity neutral`, `parity probe --check`,
`parity gen-config --check`, `parity certify` and `parity certify --check`).

## Commands

| Command | What it does |
| --- | --- |
| `reference` | Downloads the published sketch-orca 0.1.0 engine from this repository's GitHub release v0.1.0: `sketch-orca-0.1.0-orca-2.4.2-win32-x64.zip` (40,813,895 bytes) and `SHA256SUMS`, never the 611 MB source zip. It checks the zip's size and SHA-256 while it streams (`github.ts` `downloadReleaseAsset`). The SHA-256 must be the one `SHA256SUMS` lists and the one pinned in `reference.ts`. It then unpacks the zip to `<work>/parity/reference/0.1.0` and checks every file against the engine's manifest. A second run checks what is there and downloads nothing. |
| `same` | Runs the reference and the engine staged from this checkout (`<work>/dist/engine`, after `build`, `smoke` and `stage`) on every smoke case's jobs (`smoke.ts` `SMOKE_CASES`, 19 cases), one engine after the other, in one folder, so the jobs and the paths in them are the same bytes. Every output of every engine run is compared: exit code, stdout, stderr, and each file the run made, changed or removed. Each is marked identical, masked (identical after `masks.json`) or different. The command writes `<work>/parity/same.json` and exits 0 whatever it finds: a difference is a finding, not a failure. |
| `oracle` | Builds the oracle, the engine with the knobs of `oracle/patches` (`oracle/README.md`), and stages it in `<work>/oracle/dist`. It refuses to start with less than 40 GB free on the work folder's drive or in a work folder longer than `WINDOWS_WORK_PATH_MAX`, builds on the engine's dependency build (`<work>/deps`, shared, never rebuilt) with the engine's toolchain and `/MT`, writes the linker map beside the program, and stages `bin/sketch-orca-oracle.exe`, its map, the libraries it loads, the engine's resources and `oracle.json` (`oracle.ts`, `BUILDING.md`). The oracle never ships. |
| `neutral` | The neutrality gate (`neutral.ts`), on every smoke case's jobs, run as `same` runs them: (a) with no knob set, the oracle's outputs equal those of `<work>/dist/engine` after `masks.json`; (b) with `SKETCH_ORCA_ORACLE_TIME` and `SKETCH_ORCA_ORACLE_SEED` set, two oracle runs are byte for byte the same, with no mask; (c) with `SKETCH_ORCA_ORACLE_THREADS=1` and `=8` (time and seed pinned, each run dumping SOP1 files, `SOP1.md`), every run finishes, and each output or dump that differs is recorded as a T candidate, with what the dumps' RUN records say TBB reported. It writes `<work>/parity/neutral.json` and exits 1 when (a) or (b) fails, 0 when both pass. It needs `parity oracle`, and `build`, `smoke` and `stage` of this checkout. |
| `probe [--check]` | Builds the probe program (`oracle/probe/`, `probe.ts`) against the oracle's build tree, with the compile line of the engine's `cli/config_def.cpp` and the link line of `sketch-orca.exe` read from the oracle's `build.ninja`, with CMake, Ninja and `cl` into `<work>/oracle/probe-build` (no Fortran check; no `.vfproj` or `CompilerIdFortran` may appear there), and runs it hidden. It records what the engine's foundations do: the static UCRT's maths functions (each one the oracle's linker map lists, with FMA3 on and off), MSVC's casts, `printf`, `to_chars`, iostream output and parsers, the STL's sorts, heaps, searches, hashes, unordered containers and random numbers, oneTBB's split trees for P from 1 to 64, and Orca's ConfigDef (`SOP1.md`, "Probe files"). The full set goes to `<work>/parity/probes`, the committed subset to `probes/` (under 8 MB). `--check` runs it again into `<work>/parity/runs/probe-check` and exits 1 unless every file of both sets is the same bytes. It needs `parity oracle`. |
| `corpus` | Builds the corpus (`CORPUS.md`, `corpus.ts`): converts Orca's 21 OBJ test meshes to binary STL as Sketch writes `model.stl`, copies them, the 6 STL files and the 3MF as they are, from `<work>/src/tests/data`, and writes the 24 files the corpus makes (`MADE_FILES`), 73 meshes in all, into `<work>/parity/corpus/meshes`. It works out the families each case exercises from its presets, overrides and Orca's defaults (`probes/configdef.json`), and writes `corpus.json`: 276 cases, namely the smoke cases but `refusals`, each of Orca's meshes as info, layers and slice, its OBJ files read as OBJ, the made files, the Sketch-shaped jobs, a job per family the presets leave off, the placement, preset condition and orient jobs, the invalid examples and the refusals. Five cases are engine faults, recorded and held to no family (`CORPUS.md`). It needs `fetch` and `parity probe`. |
| `record [--only <regex>]` | Runs every corpus case with the oracle (time and seed pinned) at 1, 2, 8 and the machine's thread count, and with `<work>/dist/engine` five times, all in one cases folder and in the environment a client gives the engine (`record.ts`). It gives each output a class (`classes.ts`: H, R, T, N, E, B). It keeps the oracle's one-thread outputs in `<work>/parity/goldens`, and writes the front end's goldens into `goldens/front/`, `parity.json`, and `<work>/parity/record.json`. The oracle's passes run with `SKETCH_ORCA_ORACLE_DUMP` set to a fresh folder per engine run and keep the dumps of the boundaries in `KEPT_DUMPS` beside each run, as `<case>/<n>/dumps/`, named in the run's record; every other dump is removed as its run ends. The kept boundaries, in the order a run reaches them: `front-objects` (phase O2), then phase O3's `front-config`, each object step `posSlice`, `posPerimeters`, `posEstimateCurledExtrusions`, `posPrepareInfill`, `posInfill`, `posIroning`, `posContouring`, `posSupportMaterial` and `posDetectOverhangsForLift` (an `object<i>-<step>-<n>.sop1` is that step's boundary for object `i`: `dumpBoundary`, `dumpObject`), `print-psWipeTower`, `print-psSkirtBrim`, `posSimplifyPath`, `posSimplifyInfill`, `posSimplifySupportPath`, and the end-of-process boundary `print-psConflictCheck`, which oracle patch 0007 writes at the end of `Print::process()` after the conflict check (`SOP1.md`). The G-code export's dump (`print-psGCodeExport`) is phase O4's and is not kept. If the goldens would pass 8 GiB (`GOLDENS_MAX_BYTES`), the runs of domain A and every output count first, then the dumps of runs outside domain A (a case that reaches a marker family, `corpus.ts` `MARKER_FAMILIES`) are kept in order while they fit, and each one past that is kept as its SHA-256 alone (`<file>.sha256`, the portable dump's hash, listed in `goldens.json`'s `hashedDumps`; `record.ts` `hashedDumps`), which `certify` compares by hash. Each kept dump is classed across the thread counts (B when the same at each, its RUN record left out, T when not) and listed in `goldens.json`'s `dumps`; the goldens keep the one-thread pass's dumps. A pass whose program, knobs, kept boundaries and corpus are unchanged is kept, not run again. `--only` tries the matching cases in `runs/record-only` and writes nothing else. It needs `parity corpus`, `parity oracle`, and `build`, `smoke` and `stage`. |
| `envelope` | For each output `record` classed R, records which lines of the shipped engine's five runs vary, and by how much each number on them (`envelope.ts`), in `<work>/parity/envelope/envelope.json`. |
| `gen-config [--check]` | Writes `crates/orca-config/src/def_generated.rs`, orca-config's `print_config_def`, from the committed configdef probe (`probes/configdef.json`, `gen-config.ts`): every option with every field of its `ConfigOptionDef` (min and max as the bits of their `float`), the preset key lists, the legacy keys and the values the probe read them with. It refuses a probe of another format or option count, options out of order, a type, mode, GUI type or printer technology it does not know, and a default that threw. `--check` writes nothing and exits 1 while the file differs from what the probe gives. It reads only the repository and takes no run lock (it logs, as every command does, to `<work>/logs/parity-gen-config.log`). |
| `goldens --check <folder>` | Copies the oracle and the corpus to a second work folder and runs the oracle there as the goldens were run. It compares each output with the goldens, with each folder's paths written as `<case>`, `<cases>`, `<resources>` and `<work>` (`goldens.ts`). The second folder's pass keeps the dumps as `record` does, and each kept dump is compared with the goldens' too, the paths inside its strings made portable (`neutral.ts` `portableDump`). Every output or dump that still differs is an H-site (a dump's named `dumps/<file>`): it goes in `parity.json`'s `hSites` for an oracle H-patch, and its class in `record.json` becomes H. It writes `<work>/parity/goldens-check.json` and exits 0: an H-site is a finding. |
| `certify [--check] [--program <exe>]` | Runs every corpus case with `sketch-orca-rs` (`target/debug/sketch-orca-rs.exe` from `bun run rs:build`, or `--program`), in the environment a client gives an engine and with the goldens' resources, and compares every output of every run with the goldens, each side's paths made portable and the engine block made alike (its version and `implementation` are not compared; `certify.ts`). A result is compared as a JSON value (the same members in the same order, the same values), every other output byte for byte after `masks.json`, and an output `record` classed R within the envelope (each line that varies between the shipped engine's runs, each number on it between the least and greatest of them). The program runs with the golden pass's knobs (time and seed pinned, one thread) and a fresh `SKETCH_ORCA_ORACLE_DUMP` folder per run, and each dump it keeps (`KEPT_DUMPS`) is compared with the golden run's byte for byte, each side's paths made portable inside the dump's strings. An answered run must write the golden's dumps, each the same, and no other. A run the program declines (`not-ported`, exit -8) passes in no family, the front families (`version`, `config-def`, `intake`) included, so a certified family is the C++ engine's outputs in every run, with one exception: a right decline that wrote every dump of a boundary the C++ run wrote, each the same, and whose progress lines (its final `failed` line aside) are the C++ run's first lines passes the families wholly before that boundary (`BOUNDARY_FAMILIES`, one entry per kept boundary, each with every family wholly before it: `front-objects` and `front-config` pass `preset-conditions`, `model-load` and `placement`; each object and print boundary adds the families of the steps before it, in `KEPT_DUMPS`' order; the end-of-process `print-psConflictCheck` holds every family `process()` runs, phase O3's families but `layers`; no boundary holds `layers`, which comes after `process()` returns; an orient case is held to `orient` alone, as the program declines it before any boundary; an engine fault, `fault` in `corpus.json`, is held to no family), shown as the run's `boundaries` and the family's `boundaryPasses`. A decline reaches a boundary only when every kept dump of that boundary and of each one before it is the same as the golden run's, so a declined run is compared only before its decline, and a run reaches the end of `process()` only when every kept dump up to it is the same. A golden dump the goldens keep as its SHA-256 is compared by the hash of the program's portable dump. A decline that wrote a dump the C++ run did not, or one that differs, is wrong. Goldens recorded before dumps were kept are compared without them. A decline is also wrong in itself when the C++ run ended within what the program does itself (its progress shows no stage past `job`, `presets`, `model` and `arrange`, `RUST_STAGES`, and a run whose last stage is `model` or `arrange` ended there with its `failed` or `done` line; `endedWithinRust`), when it names a front family, a certified family or one `parity.json` does not have, when it names `preset-conditions` and the C++ run did not reach the presets, when the program wrote a file the C++ did not, and when it wrote to stdout or stderr anything but what the C++ run wrote there up to that point: line by line the first lines of the C++ run's stream, each line whole with its end, each side's paths made portable and the masks that name the stream applied (`streamPrefixProblem`; the C++ run went on and may have written more, the decline may not write one byte it did not). A stream the decline left empty always passes that rule, so a decline that passes a family by its boundary is not held to the lines the C++ wrote to stderr before the boundary; those lines are held by the runs the program answers whole (every `info` case) and by the `rs-*` smoke cases of `stage`. That rule judges the two streams only; a decline's exit code and progress stay with the rules above. After a run the program declines, the case goes on from the golden run's outputs (`goldenCarry`): when the golden run had the same command line, the case's folder is given the files the C++ run wrote (text with the golden's folder paths swapped for this run's, `rebaseBytes`; a zip as it is), the files it removed are removed, and so is a file only the decline made, and the case is told the C++ run's exit code and stdout. So a smoke case that makes a later run from an earlier one's outputs (pocket-pause's slice at the print Z of its layers run, imported-settings' import of its export run's `.gcode.3mf`) makes it with the job and inputs the C++ program got. The declined run is judged as it was kept; the later run is the program's own and is judged like any other, never passed for being there. The report lists those declines as `carried`. Two more lines say what the declines wrote and which answered runs hold what they do not, and neither changes a verdict or the exit status: `decline streams: <n> declines; stderr <a> both empty, <b> empty, <c> a prefix, <d> not a prefix; stdout ...` (`declineStreams`, in the report as `declineStreams`: each declined run's stderr and stdout is both empty when the C++ run's and the decline's are empty, empty when only the decline's is, a prefix when its lines are the C++ run's first lines, not a prefix otherwise; the first five not a prefix follow as `  not a prefix: <case>/<run> <stream>`), and for each family `pre-boundary holders of <family>: <k> answered runs (<case>/<run>, ...)` (`preBoundaryHolders`, in the report as `preBoundaryHolders`: the runs the program answered whole, every output and dump the golden's, of the cases that reach the family, with the first five named), which hold the lines the C++ writes before a boundary that a decline's empty stream does not. A family whose every run of every case passes is certified in `parity.json`. A flag is raised, never lowered. It refuses a program older than a crate's `Cargo.toml`, `build.rs` or `src/`, or the workspace's Cargo files or toolchain pin. It also refuses goldens that do not stand for this checkout: the staged oracle must be the one this checkout makes (the commit, `patches/`, the oracle patches and `cli/`), `neutral.json` must have passed with that oracle, and `goldens.json` must name that oracle as the one it was recorded with (`oracleChainProblems`), so a change to `cli/` cannot reach certify before `parity oracle`, `parity neutral` and `parity record` run again. It writes `<work>/parity/certify.json`; `--check` runs into `runs/certify-check`, writes `certify-check.json` and no `parity.json`. Either exits 1 when a family `parity.json` certifies does not pass, and when the decline lets a corpus case through (`declineGaps`, listed in the report): every run of a job, example or refusal case (`DECLINE_HELD_KINDS`; a smoke case's families are named for the whole case, not worked out for each of its runs) whose families (`corpus.json`; for a `job` case `familiesFor` and the presets' compatibility conditions) include one `parity.json` does not certify must be a `not-ported` decline whose `missing` names each such family, unless its C++ run ended within what the program does itself; the families are held as this same run raises them, so a family certified now needs no decline. It needs `parity corpus` and `parity record` (and `parity envelope` for R outputs). |

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
parity/corpus/                the corpus's meshes (meshes/<mesh>/<file>) and a copy of corpus.json
parity/goldens/               the oracle's one-thread outputs of the corpus (cases/<case>/<n>/) and
                              goldens.json (what made them, the oracle they were recorded with,
                              and the paths they name)
parity/record.json            parity record's report: every output and its class
parity/goldens-check.json     parity goldens --check's report: the H-sites
parity/runs/                  one folder per comparison run; same/ holds parity same's cases
                              (cases/), and each engine's recorded runs (reference/, head/:
                              <case>/<n>/ with stdout, stderr and files/); neutral/ holds parity
                              neutral's cases, its recorded runs (head/, oracle/, pinned-a/,
                              pinned-b/, threads-1/, threads-8/) and the dumps of the thread runs
                              (dumps-t1/, dumps-t8/: <case>/<n>/*.sop1); record/ holds parity
                              record's cases (cases/) and a store per pass (oracle-p<P>/,
                              engine-<n>/: <case>/<n>/ and pass.json; an oracle pass's runs keep
                              their dumps in <case>/<n>/dumps/, and oracle-p<P>-dumps/ holds them
                              while the pass runs)
parity/probes/                the probe program's full set (parity probe): crt.sop1, fmt.sop1,
                              stl.sop1, tbb.sop1, configdef.json
parity/runs/probe-check/      parity probe --check's second run (committed/, full/)
parity/envelope/              the shipped engine's own run-to-run variance
parity/same.json              parity same's report
parity/neutral.json           parity neutral's report
parity/certify.json           parity certify's report: each family's verdict and first problems,
                              the decline gaps and the carried declines (certify-check.json for
                              --check); its runs in runs/certify/ (cases/,
                              and rust/: <case>/<n>/ with the dumps in dumps/; rust-dumps/ while it
                              runs), or runs/certify-check/
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
about 6.4 MB), `corpus.json` and `CORPUS.md` (the corpus, its families and classes),
`parity.json` (every family, its classes and whether it is certified), and `goldens/front/` (the
front end's goldens: `version.json`, `config-def.json` and `intake/<case>.result.json`, about
0.5 MB). Meshes, slice goldens and the full probe set stay in the work folder.

## Tests

`bun run test` covers the harness hermetically: fakes and fixtures only, never the work folder or
the network. The tests are `test/parity-dispatch.test.ts`, `test/parity-reference.test.ts` (a fake
GitHub and fetch), `test/parity-same.test.ts`, `test/parity-oracle.test.ts` (the oracle's patches
and knobs, the refusals of `parity oracle`, the SOP1 reader and what `parity neutral` makes of runs
and dumps), `test/parity-probe.test.ts` (the map's maths functions, the compile and link lines
read from a build.ninja, the probe's configure line, a fake probe run, and the committed probe
files), `test/parity-corpus.test.ts` (the meshes and the OBJ to `model.stl` conversion, the
families a job's settings reach, the cases, a job made concrete in a case's folder, the invalid
examples moved into one, and `corpus.json`), `test/parity-classes.test.ts` (each class, paths made
portable, the thread counts, `parity.json`, the H-sites of two folders and the envelope),
`test/parity-gen-config.test.ts` (the committed file against the committed probe, the fields left
to `OptionDef::EMPTY`, the Rust literals, the refusals of a probe it does not take, and the
command's registration), `test/parity-certify.test.ts` (the engine block made alike, results as
JSON values, the envelope, a decline passing no family and when it is wrong, the C++ run's first
lines on a decline's streams and not a byte more, families, flags raised and never lowered, the
dumps and the boundaries a decline passes (cumulative, and the end of `process()` on made-up runs), what the declines wrote on their streams and the pre-boundary holders, the runs after a decline made from the golden run's
outputs and judged in full, the program's sources, the decline against the corpus cases and
parity.json as the run raises it, the oracle behind the goldens, and the command's registration),
`test/parity-record.test.ts` (the dumps a pass keeps and the boundary of each, their classes across
the thread counts, the goldens' dumps and those kept as their SHA-256 past 8 GiB, a pass's key and
a dump's paths made portable), `test/parity-goldens.test.ts` (the
dumps of two folders as H-sites), `test/github.test.ts` and `test/paths.test.ts`.
