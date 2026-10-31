# sketch-orca

`sketch-orca` is a free command-line slicer: a modified version of
[OrcaSlicer](https://github.com/OrcaSlicer/OrcaSlicer), made of Orca's slicing library (libslic3r)
and a command-line front end (`cli/`), built without Orca's GUI. It reads a job file and writes
G-code, a project file and a result file. Other programs run it as a separate program and talk to
it only through its command line and files, which `protocol/` specifies; Sketch is one such
program.

One script, `scripts/orca.ts`, takes it from Orca's GitHub release to a staged engine and its
source bundle. Every command can be run again: it skips what is already done and verified, and
stops with a message when something is wrong. It runs locally; there is no CI job.

## Licence

sketch-orca is free software: you can redistribute it and/or modify it under the terms of the GNU
Affero General Public License, version 3, as published by the Free Software Foundation (`LICENSE`,
SPDX `AGPL-3.0-only`). That is OrcaSlicer's licence: its README licenses it under "the GNU Affero
General Public License, version 3", with no "or later". It comes with no warranty, to the extent
permitted by law.

- `NOTICE` gives what sketch-orca is based on (OrcaSlicer, by the OrcaSlicer contributors, which is
  based on Bambu Studio, PrusaSlicer and Slic3r), the code of others it carries, and what was
  changed and when. `AUTHORS` names the copyright holders of the changes ("The sketch-orca
  authors" in the file headers).
- `protocol/` (the specification of the command line and files, its JSON Schemas and examples) is
  licensed under the MIT licence (`protocol/LICENSE`), so that any program may implement it.
- Every staged engine carries `LICENSES/`: OrcaSlicer's licence, `NOTICE.txt`, `AUTHORS.txt`, and
  the licences of every library built into it.

## Source offer

Whoever distributes a sketch-orca engine must offer its complete corresponding source with it
(AGPL-3.0, section 6). This repository's releases do it this way:

- Every binary release carries, in the same release, the complete source bundle that `source`
  made for that very engine: OrcaSlicer at the locked commit, this repository, the source packages
  of every library built into the engine (with `dependencies/` and `prebuilt/`), its licences, and
  a `BUILDING.md` that builds it from the bundle alone. `bundle.json` names the SHA-256 of the
  engine's manifest, so a bundle belongs to one engine.
- A bundle made with `--no-deps` or `--allow-missing` is not complete and is not published.
- The source stays available as long as the binary is offered: a release is never left with the
  binary and without its source, and taking the source down takes the binary down with it.
- A program that downloads the engine for its users points them at that release, where the source
  is offered next to the binary.
- sketch-orca runs on the user's own machine and has no network interface. A service that runs a
  modified engine for users over a network must offer those users its source (section 13).

## Profiles

Vendor profiles (printers, processes, filaments) are never committed here. The staged engine
carries `resources/profiles/` from OrcaSlicer's tree at the locked commit, as Orca ships them, and
the source bundle has them inside Orca's source archive. `smoke` writes the presets it needs from
them into the work folder at run time; the tests here use only presets they make themselves.

## Running it

From the repository root, after `bun install`:

```
bun run engine <command> [options]     # bun scripts/orca.ts <command>
bun run test                           # bun test ./test: unit tests, no network, no builds
bun run typecheck                      # tsc: type-check the scripts and tests
```

`BUILDING.md` lists the tools and versions the engine is built with.

| Command | What it does |
| --- | --- |
| `check` | Asks GitHub for the latest stable OrcaSlicer release and compares it with `orca.lock.json`. Exit 0: nothing to do (the lock is the latest release, or newer); 1: a newer stable release is out; 2: the locked tag now points at another commit; 3: the check failed. |
| `update [--write]` | Moves the lock to a newer stable release; without `--write` it prints the new lock. It never moves the lock back, or onto a moved tag (exit 2). |
| `fetch` | Shallow clone of the locked tag into `<work>/src`; the commit must be the lock's. |
| `build [--step deps\|orca]` | Applies `patches/`, builds Orca's dependencies without the GUI ones, then libslic3r and `sketch-orca` (Release, `SLIC3R_GUI=OFF`). Fetches first when the checkout is missing or at another commit. |
| `smoke [--case <id,...>]` | Slices the test models with the built engine and checks the results (see Smoke). |
| `stage [--without-smoke]` | The engine as it ships, in `<work>/dist/engine`, with a manifest. Only an engine that passed smoke is staged; `--without-smoke` stages a dev build and the manifest says so. |
| `source [--no-deps] [--allow-missing]` | The engine's complete corresponding source, in `<work>/dist/source/<name>`. `--no-deps` leaves the dependency packages and the prebuilt libraries' sources out (they stay listed with URL and SHA-256); `--allow-missing` makes the bundle even when a built dependency or a prebuilt library has no pinned source. |
| `release [--offline] [--no-deps] [--allow-missing]` | `check`, `update` (only to a newer stable release), `fetch`, `build`, `smoke`, `stage` and `source`, in order, stopping at the first failure. `--offline` leaves out `check` and `update`. |
| `env` | Prints the toolchain found and the cleaned build PATH. |

`release` is the workflow for a new engine. After a failure, fix the cause and run it again: the
finished steps skip. When a newer release is out it rewrites `orca.lock.json` in the repository;
committing that change is up to you. Nothing is committed, pushed or uploaded by these commands.
Every command but `env` logs to `<work>/logs/<command>.log`.

Exit codes (`EXIT` in `scripts/lib/run.ts`): 0 when the command did its work; `check` also exits 1
(a newer stable release is out) and 2 (the locked tag moved), and `update` and `release` stop with
2 when the locked tag moved; 3 when the command failed; 64 for a command line the pipeline does not
take (an unknown command, or `--step` with another value). The commands that write (all but
`check` and `env`) hold `<work>/run.lock`, which names their process, while they run: a second
run on the same work folder stops with exit 3 and names the first, and a lock whose process is
gone is taken over.

## Where things go

The repository:

```
LICENSE          the GNU Affero General Public License, version 3 (OrcaSlicer's LICENSE.txt)
NOTICE           what sketch-orca is based on, the code of others it carries, and what changed when
AUTHORS          the copyright holders of the changes
BUILDING.md      the tools, their versions, and the order of the commands
CONTRIBUTING.md  how to contribute: licences and the Developer Certificate of Origin sign-off
orca.lock.json   the OrcaSlicer release we build: repo, tag, commit, version, publish date, licence
patches/         our changes to Orca's build (no GUI dependencies; the front end in Orca's tree),
                 applied in order with git apply
cli/             the front end, sketch-orca (C++), built inside Orca's tree
protocol/        the engine's command line and files: specification, JSON Schemas, examples (MIT)
licenses/        licence texts we supply for dependencies whose packages lack them
scripts/         orca.ts and its modules in lib/
test/            unit tests and their fixtures
package.json     the scripts above and the development dependencies; bun.lock pins them
```

The work folder is `%LOCALAPPDATA%\SketchDev\orca-engine` on Windows and
`$XDG_DATA_HOME/SketchDev/orca-engine` (`~/.local/share/SketchDev/orca-engine`) elsewhere;
`SKETCH_ORCA_HOME` overrides it.

```
src/            OrcaSlicer at the locked commit, with patches/ applied
downloads/      the dependency source packages, one folder per dependency, kept across rebuilds
prebuilt/       the source packages of the libraries Orca ships prebuilt (GMP), kept across runs
deps/           Orca's dependency build; its install prefix is deps/OrcaSlicer_dep/usr/local
build/          Orca's build; the engine is build/sketch-orca/sketch-orca(.exe)
smoke/          the smoke run: presets, one folder per case, report.json
dist/engine/    the staged engine
dist/source/    the source bundle
logs/           one log per command
state/          what each step last did, and from which inputs
run.lock        while a command that writes runs: its process id, command and start time
```

### The staged engine

```
bin/            sketch-orca(.exe) and the libraries it loads
resources/      profiles/, info/ and flush/ from Orca's resources: what libslic3r reads at run time
LICENSES/       OrcaSlicer/LICENSE.txt (AGPL-3.0); NOTICE.txt and AUTHORS.txt (this repository's
                NOTICE and AUTHORS); deps/<Name>/ and deps_src/<name>/, the licences of the code
                built into the engine: a deps_src part whose licence is stated only in a source
                file gets that notice as <file>.notice.txt (DEPS_SRC_NOTICES in stage.ts), and
                OpenCV's notices for the code it builds in are in deps/OpenCV/third-party/;
                README.txt, which also dates our modifications
manifest.json   sketch-orca-engine/1 (protocol/schema/engine-manifest.schema.json)
```

The manifest holds the engine (name, version, `exe`, `resources` and `libraryPath`, relative to the
folder), the lock (`orca`), platform and arch, the patches with their SHA-256, the hash of the
`cli/` the engine was built from, the libraries in `bin/` (`runtime`) and those the system
provides (`system`), the smoke result, the hash of the staging inputs, the time, and every other
file with its size and SHA-256. `stage.ts` `readManifest` reads and checks it.

Before staging, `stage` checks that the checkout is at the locked commit; that the patches `build`
last applied (`state/patches.json`, names and SHA-256) are `patches/` as it is now (it compares
the patch files, not the patched sources, which `build` resets and patches again whenever they
differ); that a patch header dates our changes ("Modified for Sketch, <date>."); that `cli/`
hashes as it did when the last build started (`state/orca.json`; the build itself fails when
`cli/` changes while Ninja runs); that a full smoke run passed for this very executable (the
stamp and `smoke/report.json`); that every library the engine imports is found (Windows: the PE
import closure; the Visual C++ runtime ships next to the engine and is never taken from System32);
that every built dependency, and every `deps_src` library on the engine's link line (read from
`build/build.ninja`), has a licence text; that the link line holds no Intel IPP; that the GMP and
SoftFloat texts a Windows engine needs are there; that `NOTICE` and `AUTHORS` are there; and that
`sketch-orca version` names the locked commit. It builds the folder next to the old one and
refuses it when a staged file names the builder's home folder or user name (`__FILE__` paths; see
Toolchain). It runs the staged copy with only the system folders on its PATH (on Linux and macOS,
plus `bin/` as the library path): `version`, which proves `bin/` is complete, then smoke's
`cube-fixed` and `bambu-3mf` cases with the staged `resources/` (`STAGED_CASES`, in
`<work>/smoke/staged`), which proves the folder slices with what it ships. Then it swaps it in.

### The source bundle

`<work>/dist/source/sketch-orca-<engine version>-orca-<Orca version>-<platform>-<arch>-source/`:

```
orca/OrcaSlicer-<tag>.tar.gz   OrcaSlicer at the locked commit (git archive with no line-ending
                               conversion: each file as committed, most with LF, some, mostly
                               under resources/profiles, with CRLF)
engine/                        this repository: lock, patches, cli, protocol, licenses, scripts,
                               tests, README, NOTICE, AUTHORS, LICENSE: the files git tracks, at
                               the commit bundle.json names (no .git, node_modules or local work
                               folders; source refuses a checkout with changes that are not
                               committed or files git does not track)
dependencies/<Name>/<file>     the source packages Orca's dependency build downloaded, each
                               matched against the SHA-256 its recipe pins
prebuilt/<Name>/<file>         the source packages of the libraries Orca ships prebuilt, when
                               their binary is the one pinned (GMP 5.0.1 on Windows x64)
LICENSES/                      the staged engine's licences
BUILDING.md                    how to build the engine from the bundle alone
bundle.json                    sketch-orca-source/1: the engine it belongs to (its manifest's
                               SHA-256), this repository's commit, the lock, the patches, each package with URL and
                               SHA-256, and the libraries built from Orca's tree or shipped
                               prebuilt, with their sources
SHA256SUMS                     every other file (check with sha256sum -c SHA256SUMS)
```

The bundle is a folder; pack it to publish it next to the engine.

## Toolchain

Windows is the platform the pipeline has been run on (`BUILDING.md` has the versions):

- Visual Studio 2022 with the MSVC x64 tools, found with vswhere; the build runs in the
  environment of its `vcvars64.bat`.
- CMake and Ninja, run by absolute path: Visual Studio's own first, then `C:\Program Files\CMake`,
  then the copies Strawberry Perl ships in `C:\Strawberry\c\bin`. When CMake sits in a toolchain
  folder such as `C:\Strawberry\c`, that prefix goes to `CMAKE_IGNORE_PREFIX_PATH` so CMake does
  not find its MinGW libraries.
- Strawberry Perl's `perl.exe` (`C:\Strawberry\perl\bin`), for OpenSSL's Configure.
- git, for `fetch` and the patches.

The build PATH is made from vcvars: Strawberry's `c\bin` (gcc and MinGW libraries), Git's MSYS
folders (an MSYS perl), MSYS2 and Cygwin stay off it, and variables that steer compilers, linkers,
CMake or perl (`CC`, `CXX`, `CL`, `_CL_`, `LINK`, `_LINK_`, `CMAKE_PREFIX_PATH`,
`CMAKE_LIBRARY_PATH`, `DESTDIR`, every `*_ROOT`, `PERL5LIB`, `OPENSSL_DIR` and others) are
dropped. vcvars itself starts from an environment without the caller's `INCLUDE`, `LIB`, `LIBPATH`
and `EXTERNAL_INCLUDE`, so it does not append them. `GIT_CEILING_DIRECTORIES` is the work folder,
because Orca's dependency build decides how to apply its patches by asking git whether it is
inside a repository. `env` prints all of it.

`__FILE__` puts source paths into the binaries (Boost, NLopt and OCCT assertion and log
messages), and with them the builder's home folder. On Windows the pipeline then sets
`CL=/d1trimfile:"<work>\\" /d1trimfile:"<engine>\\"` for both builds, so `cl` cuts those prefixes
off; the dependency and Orca builds are each started over (`state/deps-env.json`,
`state/orca-env.json`) when that setting changes, because Ninja and ExternalProject do not notice
it. Linux and macOS get no such flags (passing CFLAGS changes what GMP's configure picks): build
release engines there with `SKETCH_ORCA_HOME` outside your home folder. Either way `stage` refuses
an engine that still names your home folder or user name.

Linux and macOS have code paths but have not been tried yet: the build PATH holds cc, c++, make,
m4, autoreconf, automake and libtool (glibtoolize on macOS), `ldd` or `otool -L` find the runtime
libraries, and the engine runs with `LD_LIBRARY_PATH` or `DYLD_LIBRARY_PATH`. The macOS build
passes no deployment target yet.

### Environment

| Variable | Use |
| --- | --- |
| `SKETCH_ORCA_HOME` | The work folder. |
| `SKETCH_ORCA_VS` | The Visual Studio install folder, instead of asking vswhere. |
| `SKETCH_ORCA_CMAKE`, `SKETCH_ORCA_NINJA`, `SKETCH_ORCA_PERL`, `SKETCH_ORCA_GIT` | The programs to use, by path. |
| `SKETCH_ORCA_JOBS` | Compile jobs for the Orca build (default: one per core, at most one per 2.5 GB of memory). |
| `SKETCH_ORCA_DEPS_JOBS` | Dependencies built at the same time (default 3; each builds in parallel itself). |
| `SKETCH_ORCA_SMOKE_TIMEOUT_MS` | Time limit of one smoke case (default 15 minutes). |
| `GITHUB_TOKEN` | Sent to the GitHub API by `check`, `update` and `release` when set, for a higher rate limit. |

## Smoke

`smoke` writes the presets it needs from Orca's own profiles (`scripts/lib/profiles.ts` flattens
the inheritance), runs the engine on each case and checks the output (`scripts/lib/gcode.ts`). The
cases are `SMOKE_CASES` in `scripts/lib/smoke.ts`:

- `version`: the engine names the locked Orca release.
- `cube-fixed`: a 20 mm cube at 0.2 mm with a 0.25 mm first layer has exactly the expected layers,
  and the length `stats` gives each feature is the length the G-code prints of it (within 3%).
- `sphere-adaptive`: adaptive layer height on a sphere varies within the printer's limits.
- `pause-at-layer`: a pause at a print Z appears once, before that layer, as the machine's pause G-code.
- `layers-only`: `layers` returns the layer ranges, names no outputs and writes no G-code.
- `config-def`: `config-def` lists the settings with their types and enum values, and the obsolete
  keys (`obsolete`, sorted; `renamed` gives the key Orca reads a renamed one as; `stillDefined`
  the ones Orca still defines): none of them is an option, and they are the keys Orca's
  `handle_legacy` names in the checkout the engine was built from (every key it ignores, every key
  it reads that is not an option, no other). Each key Orca still defines must be one
  `handle_legacy` ignores: a live setting whose value it fixes (`wall_direction`) would otherwise
  pass as obsolete if every probe value of `config-def` happened to drop or rename it.
- `bambu-3mf`: an X1 Carbon job writes a `.gcode.3mf` holding `Metadata/plate_1.gcode`, and every
  part its relationships name is in the archive.
- `pocket-pause`: in a box with a closed pocket, the pause goes before the first layer covering it.
- `info`: `info` returns the cube (named after its file, one instance, its triangles and size), the
  bed of the machine preset and the preset names, and slices nothing.
- `filaments`: two filaments, with no `filament_colour` given: a change to filament 2 at 5.05 mm
  prints its layers with T1; a change on the first layer, and support on filament 2 where no
  support is needed (a prime tower planned but not made, patch 0004), slice instead of crashing.
- `z-offset`: the cube with the printer's `z_offset` at 0.3 mm and at -0.2 mm (an override) has
  the expected layers, its layer tags and result at print Z, and every extrusion at its layer's
  print Z plus the offset, the last one in the top layer. Orca's G-code processor takes `z_offset`
  back out of the positions it stores, so the engine's check that the G-code reaches the top
  (`cli/run.cpp`) compares print Z with print Z; it used to add `z_offset` again and refused a
  raised nozzle. Both offsets are larger than that check's slack (half the last layer plus
  0.01 mm, 0.11 mm on the cube), so a check that adds `z_offset` refuses the raised nozzle and
  one that takes it off refuses the lowered one.
- `override-keys`: overrides whose key has capital letters (`required_nozzle_HRC`, a filament
  setting) or is an old name Orca renames for its value (`top_one_wall_type` `topmost`, read as
  `only_one_wall_top` = 1) are applied: the cube slices and the G-code's config block gives their
  values.
- `crash-result`: an engine that crashes inside Orca's code (`SKETCH_ORCA_TEST_CRASH`, read by
  `cli/common.cpp`) still writes its result: `internal-error`, exit code 255, no outputs.
- `refusals`: a missing model (`file-not-found`, -3), an unknown setting, also one with capital
  letters (`Layer_Height`; `setting-invalid`, -2), a key of the preset files as an override
  (`Inherits`; `job-invalid`, -2), an old name with the value Orca drops it for
  (`top_one_wall_type` `none`; `setting-invalid`, -2), a pause below the first layer and two
  custom G-codes at one print Z (`custom-gcode-invalid`, -2), a cube bigger than the bed
  (`arrange-failed`, -21, object 0), a flat model and a lone triangle (an OBJ, since Orca's STL
  loader drops a lone facet; `model-unreadable`, -6, object 0), layer profiles that do not start
  at z 0 or go above the nozzle (`layer-profile-invalid`, -2, object 0), and a G-code folder that
  cannot be made (`output-failed`, -13, with no layers, printZ or outputs) are refused with that
  error, exit code and object, a message naming the culprit where there is one, and no G-code.
  Jobs that `scripts/lib/job.ts` refuses itself are written unchecked, so the engine's own check
  is what is tested.

Every engine run must exit with the code its result gives (`exitCode`); parents compare the low
byte, since Bun on Windows and POSIX systems report only 8 bits of a negative exit code.

The G-code may not be reproducible byte for byte across copies of the engine. The same job sliced
by the same executable gives the same G-code every time from one place. An earlier build of
0.1.0 gave a slightly different toolpath from another folder: `cube-fixed` from
`build/sketch-orca` estimated 590.78 s and 1315.66 mm of filament, from `dist/engine/bin`
590.52 s and 1315.59 mm, a few short extrusions near the corners. Thread timing was not the cause
(pinned to one CPU, each place kept its own result), and the cause was not found. The build
staged on 2026-10-30 gives the same numbers from both places. Smoke and stage check properties of
the result, never its bytes, and nothing should compare or cache G-code by its hash across engine
installs.

The report is `<work>/smoke/report.json`; a full run (all cases) that passes also writes the stamp
`state/smoke.json`. `stage` takes an engine only when both say that a full run of the current
cases (`SMOKE_VERSION`) passed for the same executable: a full run removes the old stamp before it
starts, and any failing run removes it too.

## What the engine leaves as Orca has it

- Unknown keys in preset files are skipped without a warning, as Orca's own loader skips them.
  Orca's bundled profiles use about 270 keys, in some 21,800 places, that the settings catalogue
  of 2.4.2 (`config-def`) does not list: old names Orca converts, Bambu Studio settings, misspelt
  keys such as `layer_heigth`. A warning would come with nearly every job. Unknown keys in the
  job's `overrides` are refused (`setting-invalid`).
- An override's key has the shape of Orca's keys: a letter, then letters, digits and `_`, capital
  letters too (Orca has `required_nozzle_HRC`); whether Orca has the setting is the engine's to
  say (`setting-invalid`). The keys of the preset files (`type`, `name`, `from`, `inherits`,
  `compatible_printers`) are refused as overrides in any letter case (`job-invalid`), as Orca's
  loader reads the first four in any case. `scripts/lib/job.ts` and `cli/job.cpp` take the same
  keys.
- Obsolete keys are Orca's: those its `PrintConfigDef::handle_legacy` drops or renames whatever
  their value. `cli/CMakeLists.txt` reads the keys `handle_legacy` names from the checkout's
  `PrintConfig.cpp` at configure time, and `config-def` asks Orca's own `handle_legacy` which of
  them it never keeps: they are its `obsolete` list (87 at 2.4.2), left out of `options` even
  where Orca still defines them (`stillDefined`: `silent_mode`, `tree_support_with_infill`), and
  `renamed` names the key Orca reads each renamed one as (43). As an override, a dropped key is
  refused (`setting-invalid`), and a renamed one is applied under its new name, as Orca applies
  it, for the values Orca renames it for. Two renames depend on the value: `prime_tower_rib_wall`
  becomes `wipe_tower_wall_type` = `rib` only when it is 1, and `top_one_wall_type` becomes
  `only_one_wall_top` = 1 unless it is `none`; with any other value Orca drops them, and they are
  refused (`setting-invalid`) like a dropped key.
- A pause's note, and for a colour change with one filament "Change the filament to <colour>", is
  stored as Orca's pause message, but Orca 2.4.2 writes no message line into the G-code (its
  `M117` line is commented out): the printer shows no text, and the note is only in a
  `.gcode.3mf`'s `Metadata/custom_gcode_per_layer.xml`. A client that wants the note seen shows it
  itself, from its own job. The engine adds no display command of its own, since firmwares do not
  take one alike (`M117`, `M118`, Klipper's `RESPOND` only with `[respond]` set up).

## Open items

These are for the maintainers to decide, and some for a lawyer, before the first public release
of an engine binary.

- **The Visual C++ runtime (Windows).** The engine is built with MSVC against the dynamic CRT
  (`/MD`), and `stage` ships the runtime DLLs it imports (`msvcp140.dll`, `vcruntime140.dll`,
  `vcruntime140_1.dll` and the like, from Visual Studio's redistributable folder through CMake's
  `InstallRequiredSystemLibraries`, without the UCRT) next to it in `bin/`. The FSF's FAQ
  ("I'm writing a Windows application with Microsoft Visual C++ (or Visual Basic) and I will be
  releasing it under the GPL. Is dynamically linking my program with the Visual C++ (or Visual
  Basic) runtime library permitted under the GPL?",
  https://www.gnu.org/licenses/gpl-faq.html#WindowsRuntimeAndGPL) says:

  > You may link your program to these libraries, and distribute the compiled program to others.
  > When you do this, the runtime libraries are "System Libraries" as GPLv3 defines them. That
  > means that you don't need to worry about including their source code with the program's
  > Corresponding Source. GPLv2 provides a similar exception in section 3.
  >
  > You may not distribute these libraries in compiled DLL form with the program. To prevent
  > unscrupulous distributors from trying to use the System Library exception as a loophole, the
  > GPL says that libraries can only qualify as System Libraries as long as they're not
  > distributed with the program itself. If you distribute the DLLs with the program, they won't
  > be eligible for this exception anymore; then the only way to comply with the GPL would be to
  > provide their source code, which you are unable to do.

  The build is unchanged for now; the options are:
  1. Stop shipping the DLLs, and require Microsoft's Visual C++ Redistributable (x64) on the
     user's machine: the program that installs the engine checks for it, or asks the user to
     install it from Microsoft. The DLLs then stay System Libraries, as the FAQ describes.
  2. Build with the static CRT (`/MT`, `CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded`), for the engine
     and every dependency, so no runtime DLL ships. Whether statically linked runtime code still
     counts as part of a System Library is itself a question for counsel, and every dependency's
     build has to take the setting.
  3. Keep shipping them app-locally, as now, only with counsel's view that this is compatible
     with the AGPL despite the FAQ.
- **GMP (Windows).** Its source is pinned and bundled (`BUILDING.md`, Prebuilt libraries). Not
  known: the exact configure command line and build scripts CGAL used for the DLL (gmp.h gives
  the compiler and flags only); a source for any other GMP binary (arm64, x86, or a changed DLL
  in a later Orca release, which `source` then refuses); and the libgcc and MinGW-w64 runtime code
  linked statically into the DLL by GCC 4.4.5 (under the GCC Runtime Library Exception and the
  MinGW-w64 runtime's permissive terms; counsel may confirm that no further source is needed).
- **MPFR (Windows).** Orca ships MPFR 3.0.0 prebuilt too; the engine does not link it, so it is
  neither shipped nor bundled. If a build ever links it, its source must be pinned like GMP's.
- **OpenSSL.** Orca's dependency build pins OpenSSL 1.1.1w (`deps/OpenSSL`, the recipe excerpt in
  `test/bundle.test.ts`), and libslic3r includes `openssl/md5.h` (`Utils.hpp`, `bbs_3mf.cpp`), so
  the engine very likely links OpenSSL's libcrypto. Not checked yet: whether it does, and how
  (statically into the executable or as a library in `bin/`), and that its licence text is staged
  in `LICENSES/deps`. OpenSSL 1.1.1 is under the OpenSSL and SSLeay licences, whose advertising
  clauses the FSF holds incompatible with the GNU GPL family, the AGPL included, unless the
  copyright holders of the GPL'd code grant an exception; OpenSSL 3.0 and later are under the
  Apache License 2.0, which is compatible with version 3. Stock OrcaSlicer's releases are built the
  same way. The options: confirm the link and stage its licence; build the dependencies against
  OpenSSL 3 (a change to Orca's recipe, as a patch here); or counsel's view on shipping 1.1.1w.
- **libnoise.** The package Orca pins dropped libnoise's `COPYING.txt`. Staging ships the grant
  from its sources (`src/noise.h`, LGPL 2.1 or later) and the LGPL 2.1 text from `licenses/`.
- **The toolchain record.** `BUILT_WITH` (`scripts/lib/bundle.ts`, and so each bundle's
  `BUILDING.md`) states the versions the engine has been built with, by hand. Recording the
  versions each build actually used, in the manifest or `bundle.json`, would make that exact.
- **The copyright holder.** The file headers name "The sketch-orca authors", and `AUTHORS` lists
  the git identity `kevin-t2`. Put the legal name of the person or company that holds the
  copyright there, as wanted.
- **Questions for a lawyer**, before the first public engine release:
  - whether running the engine as a separate program through this command line and its JSON
    files keeps a program that runs it a separate work (an "aggregate"), as `protocol/` intends;
  - the Visual C++ runtime DLLs (above);
  - who holds the copyright in the changes, and the name to give for it (above);
  - "version 3" only, as OrcaSlicer's README grants it, against the "or later" of some of the code
    Orca carries (ArcWelderLib; libvgcode, which the engine does not build);
  - the OrcaSlicer name and any trademark in it, in the name "sketch-orca" and in a client's
    interface;
  - whether any vendor profile in Orca's `resources/profiles` carries terms of its own;
  - whether `protocol/` may be MIT-licensed while it names Orca's setting keys and the exit codes
    of Orca's command line.
