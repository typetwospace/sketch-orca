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
