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
and `~/.local/share/SketchDev/orca-engine` elsewhere; `SKETCH_ORCA_HOME` puts it somewhere else.
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

## OpenSSL

libslic3r computes MD5 sums with OpenSSL's libcrypto, which the engine links statically. OrcaSlicer
2.4.2's recipe pins OpenSSL 1.1.1w, whose licences the FSF holds incompatible with the GNU AGPL;
patch 0006 builds OpenSSL 3.5.9 (Apache License 2.0) instead, from the OpenSSL project's release
archive `openssl-3.5.9.tar.gz` (SHA-256
603f5602e2eef00d77fbd429d34dcd5822bb301757a1bc9cdb24c670f1eb859a), which the source bundle carries
in `dependencies/OpenSSL/`.

## Prebuilt libraries

On Windows, OrcaSlicer's tree ships GMP 5.0.1 and MPFR 3.0.0 as prebuilt DLLs (`deps/GMP/gmp`,
`deps/MPFR/mpfr`) instead of building them. The engine links GMP (as `bin/libgmp-10.dll`) and not
MPFR.

GMP's exact source is known: OrcaSlicer's `deps/GMP/gmp` at v2.4.2 is byte for byte the GMP of
CGAL's win64 auxiliary libraries, a MinGW-w64 build of the GMP 5.0.1 release archive.
`source` downloads that archive, `gmp-5.0.1.tar.bz2` (2,006,109 bytes, SHA-256
a2a610f01fd3298dc08c87bf30498c2402590e1bcb227fc40b15ee6d280939fb), from
https://gmplib.org/download/gmp/gmp-5.0.1.tar.bz2 or https://ftp.gnu.org/gnu/gmp/gmp-5.0.1.tar.bz2,
keeps it in `<work>/prebuilt/GMP/`, and puts it in the bundle as `prebuilt/GMP/gmp-5.0.1.tar.bz2`,
but only when the engine's `bin/libgmp-10.dll` is the DLL that source was pinned for (SHA-256
fec43cb18ce0ec8a5dd6ad1db745747167310ccb92b5acff0d445a8b3013f009). For any other GMP binary the
bundle's `BUILDING.md` says that its source is not in the bundle, and `source` stops unless given
`--allow-missing`. `PREBUILT_SOURCES` in `scripts/lib/bundle.ts` records the pin and how it was
checked; `README.md` (Open items) says what is still not known about how that DLL was built.
Until the bundle has a recipe that rebuilds that DLL from the archive (the pin's `recipe`),
`source` stops for GMP too unless given `--allow-missing`, and the bundle says it is not complete.

The bundle's Orca archive leaves out the binaries of OrcaSlicer's tree that the engine's build does
not use (the WebView2 loader, the programs in `tools/`, and GMP and MPFR for other architectures;
`orcaArchiveExcludes`). It keeps MPFR's header, import library and DLL for the engine's
architecture, which Orca's Windows dependency build copies; their source is not pinned yet, so
`source` stops unless given `--allow-missing`.

## From a source bundle

A source bundle holds OrcaSlicer at the locked commit, this repository as `engine/`, the
dependency source packages, the GMP source and the licences. Its own `BUILDING.md`, written by
`source` for that engine, gives every command to build it from the bundle alone, without network
access: unpack Orca, apply the patches, build the dependencies from `dependencies/`, then the
engine.
