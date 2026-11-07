# Building sketch-orca

sketch-orca is built by one script, `scripts/orca.ts`, which takes it from OrcaSlicer's GitHub
release (the tag and commit in `orca.lock.json`) to a staged engine folder and its source bundle.
`README.md` describes every command; this file says what you need and in what order to run them.

## Tools

The engine has been built on Windows x64 with these; other versions may work, these are known to:

- Bun 1.2.15, which runs the pipeline scripts and the tests, and git.
- Visual Studio Community 2022 17.12 with the MSVC x64 tools 14.42.34433 (toolset v143) and the
  Windows SDK 10.0.22000.0. The pipeline finds Visual Studio with vswhere and builds in the
  environment of its `vcvars64.bat`.
- CMake 3.29.2 and Ninja 1.12.0: the copies Strawberry Perl ships in `C:\Strawberry\c\bin`, or
  Visual Studio's own, or those in `C:\Program Files\CMake`, run by absolute path. The CMake files
  ask for CMake 3.13 or later.
- Strawberry Perl 5.42.3.1 (64-bit), for OpenSSL's build.
- MSYS2 (base system of 2026-09-27) with the GCC 16.2.0 of its mingw64 environment (mingw-w64 14.0.0, binutils 2.47),
  which builds GMP and MPFR on Windows (patch 0007). You do not install it: `build` fetches its
  pinned packages and unpacks them into the work folder (see "GMP and MPFR on Windows").
- Rust 1.94.1, for the Rust translation in `crates/` only (see "The Rust translation" below); the
  engine does not need it.
- Linux and macOS: a C and C++ compiler (gcc or clang), make, autoconf, automake, libtool, m4 and
  perl. The scripts have code paths for them, but they have not been tried yet.

These versions are also written into each source bundle's `BUILDING.md` (`BUILT_WITH` in
`scripts/lib/bundle.ts`); a test checks that this file names the same ones. `bun run engine env`
prints the toolchain the pipeline finds on your machine, and the cleaned PATH it builds with.

## Setting up

```
git clone <this repository> sketch-orca
cd sketch-orca
bun install          # TypeScript and the type definitions, for the tests and the type check
bun run test         # unit tests: no network, no builds
bun run typecheck
```

The repository keeps each file's line endings as committed (`.gitattributes` holds `* -text`).

## Building the engine

```
bun run engine fetch    # clones the locked tag from GitHub and checks its commit
bun run engine build    # applies patches/, builds Orca's dependencies without the GUI ones, then libslic3r and sketch-orca
bun run engine smoke    # slices the test models with the built engine and checks the results
bun run engine stage    # the engine folder as it ships, with its manifest
bun run engine source   # the complete corresponding source of that engine, as a folder to publish next to it
```

or `bun run engine release --offline`, which runs them in that order and stops at the first
failure. Every step can be run again: it skips what is done and verified.

The work folder, where all of this happens, is `%LOCALAPPDATA%\SketchDev\orca-engine` on Windows
and `~/.local/share/SketchDev/orca-engine` elsewhere; `SKETCH_ORCA_HOME` puts it somewhere else
(on Windows, a folder whose path has at most 65 characters: Visual C++ takes no path over 259
characters, and the dependency build writes files up to 183 characters below the work folder).
The staged engine is `<work>/dist/engine`, the source bundle `<work>/dist/source/<name>`.

The first `build` downloads and builds OrcaSlicer's dependencies (Boost, OpenCV, OCCT, CGAL and
others); later builds reuse them. `SKETCH_ORCA_JOBS=<n>` holds both builds to n compile jobs at a
time: the dependencies are then built one at a time, each with MSBuild building one project at a
time and `cl /MP<n>`.

On Windows everything is compiled with MSVC's static runtime library (`/MT`): `build` configures
the dependencies with `-DDEP_MSVC_STATIC_RUNTIME=ON` (patch 0005) and Orca with
`-DCMAKE_POLICY_DEFAULT_CMP0091=NEW -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded`, so neither the
engine nor the DLLs it loads import a Visual C++ runtime DLL, and none is shipped. `stage` refuses
an engine that does.

## No window, no IDE

No build step opens a window: every program the pipeline starts runs hidden, and none of them may
start Visual Studio's IDE (`devenv.exe`). The engine is built with Ninja. Orca's dependency build
configures each CMake dependency with a Visual Studio generator, though, and with one of those
CMake looks for a Fortran compiler by writing a Fortran project
(`CMakeFiles/<version>/CompilerIdFortran/CompilerIdFortran.vfproj`) and running `devenv.exe` on it.
`check_language(Fortran)`, `enable_language(Fortran)` and a `project()` that names Fortran all do
it. Eigen 5.0.1 runs `check_language(Fortran)` in `test/` and `lapack/`, and on 2026-11-06 that put
Visual Studio on the screen. So on Windows `build` configures the dependencies with

```
-DDEP_CMAKE_OPTS:STRING=-DCMAKE_Fortran_COMPILER:STRING=NOTFOUND;-DBUILD_TESTING:BOOL=OFF;-DEIGEN_BUILD_TESTING:BOOL=OFF
```

(`WIN32_DEP_CMAKE_OPTS` in `scripts/lib/cmake.ts`). Orca's `deps/CMakeLists.txt` passes
`DEP_CMAKE_OPTS` to every CMake dependency, ahead of the dependency's own arguments:

- A defined `CMAKE_Fortran_COMPILER` makes `check_language` skip. `NOTFOUND` reads false, so
  Eigen's `lapack/` goes on without Fortran, as it did when the check found no compiler. It is
  typed STRING because CMake would turn a FILEPATH into an absolute path, which reads true.
- Eigen is built without its tests (`BUILD_TESTING`, `EIGEN_BUILD_TESTING`), which also leaves out
  its BLAS tests, which enable Fortran.

No other dependency asks for Fortran. None of this is in the engine's code or its patches.

`build` holds itself to it. After the dependency build's configure step, after the dependency
build, and after the engine's build, a `CompilerIdFortran` folder or a `.vfproj` file anywhere in
`<work>/deps` or `<work>/build` fails the build and names them (`scripts/lib/noide.ts`). A failed
dependency build is not recorded as finished; remove the cause and run `build --fresh`. On Windows,
MSBuild runs with `MSBUILDDISABLENODEREUSE=1`, so no MSBuild worker outlives the build. Any other
C++ build made from this repository (a second work folder) configures its dependencies the same way
and runs the same check; the parity oracle builds on the engine's dependency build, with Ninja, and
runs the check on its own build folder (see "The parity oracle").

## The Rust translation

`crates/` (one Cargo workspace at the root, described in `README.md`) is built and tested apart
from the engine, with Rust and the same Visual Studio environment:

- Rust 1.94.1 (`rust-toolchain.toml`), target `x86_64-pc-windows-msvc`, with rustfmt and clippy,
  installed by [rustup](https://rustup.rs) at user level (`rustup toolchain install 1.94.1
  --profile minimal -c rustfmt -c clippy`). cargo and the rustup proxies are found in
  `%CARGO_HOME%\bin` or `%USERPROFILE%\.cargo\bin`.
- The environment of the Visual Studio above, pinned further than the engine's build pins it:
  `vcvars64.bat 10.0.22000.0 -vcvars_ver=14.42.34433`, that is MSVC 14.42.34433 and Windows SDK
  10.0.22000.0 of Visual Studio 17.12. The Rust program links the static C runtime (`+crt-static`,
  `.cargo/config.toml`), so `link.exe` takes the same `libucrt.lib` the C++ engine links, and the
  program imports no Visual C++ runtime DLL.
- Dependencies from crates.io only, each pinned exactly in `Cargo.toml` (rayon, windows-sys, sha2)
  and fixed by the committed `Cargo.lock`; every build is `--locked`.

```
bun run rs:build            # cargo build --locked, in the pinned environment
bun run rs:test             # cargo test --locked
bun run rs:test:release     # cargo test --locked --release (the probe gate's release half)
bun run rs:clippy -- -D warnings
bun run rs:fmt:check
```

`scripts/rust.ts` runs them and stops (exit 3) when the Visual Studio, MSVC, Windows SDK or rustc
found is not the pinned one, naming which. The pipeline needs Rust too from 0.2.0: `stage` builds
`sketch-orca-rs` itself (`cargo build --locked --release`) and stages it beside the engine, and
`source` vendors its crates (`cargo vendor --locked`, `bun scripts/rust.ts vendor`) into the
bundle's `rust-vendor/`, whose `BUILDING.md` builds it with no network. A first build of a new dependency set fetches the crates
from crates.io once: run `cargo generate-lockfile` (or `cargo update -p <crate>`) when a
dependency pin changes, which is a task of its own (`CONTRIBUTING.md`), and commit `Cargo.lock`.
Builds go to `target/`.

## The parity oracle

The oracle is the engine with the knobs of `parity/oracle/patches` (`parity/oracle/README.md`): a
C++ build for the parity harness that never ships. It is built on Windows x64 only, after the
engine's `build`:

```
bun run engine build            # the dependencies and the engine's checkout the oracle builds on
bun run engine parity oracle    # the oracle, staged in <work>/oracle/dist
bun run engine smoke            # then stage: parity neutral compares the oracle with dist/engine
bun run engine stage
bun run engine parity neutral   # exit 0 when the oracle is neutral, 1 when it is not
```

`parity oracle` refuses to start with less than 40 GB free on the work folder's drive
(`ORACLE_MIN_FREE_BYTES` in `scripts/lib/parity/oracle.ts`), or in a work folder longer than the 65
characters `build` takes (or one where a parity folder leaves no room below it). The 40 GB leave
room for what it writes, about 6 GB for OrcaSlicer 2.4.2 (the clone, about 0.5 GB, its build, about
5.4 GB, and the staged copy), with the margin a failed or repeated build and the parity runs need.

It clones `<work>/src` at the locked commit into `<work>/oracle/src` (a local clone: git shares the
objects), copies `cli/` to `sketch-orca/cli` there, and applies `patches/` and then the oracle's
patches. It builds against the engine's dependency build, `<work>/deps`, which it shares and never
rebuilds: when `build` would build the dependencies again (other patches of `deps/`, another
configuration, CMake or compiler settings, a changed prefix), it stops and says to run `build`. It
configures `<work>/oracle/build` as `build` configures the engine (Ninja, Release, the same
toolchain, MSVC's static runtime `/MT`, precompiled headers), plus the linker's map
(`-DCMAKE_EXE_LINKER_FLAGS_INIT=/MAP`, checked in the cache), and builds it with at most 8 Ninja
jobs (`SKETCH_ORCA_JOBS` lowers that). It is held to "No window, no IDE" as the engine is: after
configuring and after building, a `CompilerIdFortran` folder or a `.vfproj` file in
`<work>/oracle/build` fails it. It stages `<work>/oracle/dist`: `bin/sketch-orca-oracle.exe`,
`bin/sketch-orca-oracle.map`, the libraries the program loads, `resources/` (profiles, info, flush)
and `oracle.json`, which records what it was built from.

Run again, it keeps what still holds: a change to the patches or to `cli/` makes the new tree
beside the clone and writes only the files that differ into it, so Ninja rebuilds only what they
touch; another compiler environment or dependency build starts `<work>/oracle/build` over.

`bun run engine parity probe` builds the probe program (`parity/oracle/probe/`) after `parity
oracle`, into `<work>/oracle/probe-build`. It compiles with the compile line of the oracle's own
`config_def.cpp` and links with the oracle's link line, both read from `<work>/oracle/build`'s
`build.ninja` (`scripts/lib/parity/probe.ts`), so it runs the code the engine runs. It is held to
"No window, no IDE" too: CMake configures it with Ninja for C++ only, `CMAKE_Fortran_COMPILER` set
to `NOTFOUND`, `BUILD_TESTING` and `EIGEN_BUILD_TESTING` off, and it builds with 2 Ninja jobs; a
`CompilerIdFortran` folder or a `.vfproj` file in `<work>/oracle/probe-build` before or after the
build fails it. The program runs hidden, with the oracle's library folders on `PATH`.

## OpenSSL

libslic3r computes MD5 sums with OpenSSL's libcrypto, which the engine links statically. OrcaSlicer
2.4.2's recipe pins OpenSSL 1.1.1w, whose licences the FSF holds incompatible with the GNU AGPL;
patch 0006 builds OpenSSL 3.5.9 (Apache License 2.0) instead, from the OpenSSL project's release
archive `openssl-3.5.9.tar.gz` (SHA-256
603f5602e2eef00d77fbd429d34dcd5822bb301757a1bc9cdb24c670f1eb859a), which the source bundle carries
in `dependencies/OpenSSL/`.

## GMP and MPFR on Windows

CGAL needs GMP and MPFR. On Linux and macOS, OrcaSlicer 2.4.2 builds them from their release
archives: GMP 6.2.1 (with Orca's `0001-GMP_GCC15.patch`) and MPFR 4.2.2. On Windows it copies
prebuilt DLLs from its tree instead (`deps/GMP/gmp`, `deps/MPFR/mpfr`: CGAL's MinGW-w64 builds of
GMP 5.0.1 and MPFR 3.0.0, whose build recipe is not published). Patch 0007 builds the same two
archives on Windows too, pinned by the same SHA-256s (GMP
eae9326beb4158c386e39a356818031bd28f3124cf915f8c5b1dc4c7a36b4d7c, MPFR
9ad62c7dc910303cd384ff8f1f4767a655124980bb6d8650fe62c815a231bb7b), when the dependency build is
configured with `-DDEP_MINGW_ROOT=<msys64>`:

- GMP and MPFR build with GCC, not MSVC. `deps/MinGW/build.sh` runs their own configure scripts
  and makefiles with the bash, make and m4 of an MSYS2 tree and the GCC of its mingw64 environment,
  as DLLs (`libgmp-10.dll`, `libmpfr-6.dll`), and runs their test suites (`DEP_MINGW_CHECK`, on by
  default). `deps/MinGW/MinGW.cmake` then makes MSVC import libraries (`gmp.lib`, `mpfr.lib`) from
  the `.def` files the builds write, with `lib.exe`.
- The DLLs load only Windows' own DLLs (`msvcrt.dll`, the C runtime CGAL's DLLs load too, and
  MPFR also GMP's): libgcc is linked in statically. They carry no time stamp and no symbols.
- GMP 6 keeps GMP 5's ABI and DLL name. The engine loads `libgmp-10.dll` and not MPFR.

`build` passes `-DDEP_MINGW_ROOT` itself. The toolchain is pinned in `scripts/lib/mingw.ts`: the
MSYS2 base system archive of 2026-09-27 and 22 MSYS2 packages (make, m4 and diffutils from the msys
repository; `mingw-w64-x86_64-gcc` 16.2.0 and the packages it depends on from mingw64), each with
its size and SHA-256. `build` downloads them once into `<work>/mingw/downloads`, checks them, and
unpacks them with Windows' own `tar.exe` into `<work>/mingw/msys64`. Nothing is installed and
nothing is written outside the work folder. A later `build` keeps the unpacked tree while its files
are the ones it unpacked (`state/mingw.json`).

The source bundle carries the GMP and MPFR release archives in `dependencies/GMP` and
`dependencies/MPFR`, like every other dependency package. Its `BUILDING.md` lists the toolchain's
packages with their URLs and SHA-256s. `stage` ships the licences of the toolchain code GCC links
into the DLLs (the mingw-w64 runtime, and libgcc's GPL 3 with the GCC Runtime Library Exception) in
`LICENSES/MinGW-w64/`.

The bundle's Orca archive leaves out the binaries of OrcaSlicer's tree, none of which the engine's
build uses (`orcaArchiveExcludes`): the WebView2 loader, the programs in `tools/`, and the prebuilt
GMP and MPFR DLLs and import libraries. Their headers and READMEs stay. `source` stops unless given
`--allow-missing` when the archive would keep any other binary, or when a file of the staged
engine is byte for byte a binary of the tree (such as the prebuilt GMP, if the dependencies were
built without `-DDEP_MINGW_ROOT`). The bundle then says it is not complete.

## From a source bundle

A source bundle holds OrcaSlicer at the locked commit, this repository as `engine/`, the
dependency source packages (GMP's and MPFR's among them) and the licences. Its own `BUILDING.md`, written by
`source` for that engine, gives every command to build it from the bundle alone, without network
access, apart from the MinGW-w64 toolchain's packages on Windows: unpack Orca, apply the patches,
unpack the toolchain, build the dependencies from `dependencies/`, then the engine.
