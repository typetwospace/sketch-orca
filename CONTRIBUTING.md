# Contributing to sketch-orca

sketch-orca does not accept outside contributions: pull requests, patches and issues are closed.
It is free software all the same. Under the GNU Affero General Public License, version 3
(`LICENSE`), you may study it, change it, build it and share your own modified version, for
example in a fork of your own, as long as you keep to that licence (and to the MIT licence for
`protocol/`).

## Changes made in this repository

The maintainers follow these rules for their own changes:

- Run `bun run test` and `bun run typecheck`. A change to `cli/` or `patches/` also needs a build
  and a passing `bun run engine smoke`.
- Keep each file's line endings (`.gitattributes` holds `* -text`; the scripts, `cli/` and the
  patches use LF).
- Date every change to OrcaSlicer's files, as section 5(a) of the licence asks: a patch's header
  (the lines before its diff) has a title line, `sketch-orca patch NNNN: ...`, and then the line
  "Modified for Sketch, <date>." (the form `stage` reads into `LICENSES/README.txt`), as the
  patches here do, and the lines it adds carry a dated comment. A file that takes code from
  OrcaSlicer says so under "Carries OrcaSlicer code", naming the OrcaSlicer file and the date.
- A change to the commands or files the engine reads and writes changes `protocol/`: the
  specification, the schemas and their examples, which `test/protocol.test.ts` checks against the
  engine's validators.
- Rust code (`crates/`, the translation described in `README.md`) follows these rules as well:
  - Run `bun run rs:build`, `bun run rs:test`, `bun run rs:clippy -- -D warnings` and
    `bun run rs:fmt:check` besides the two above, and format with `bun scripts/rust.ts fmt`.
    Builds are `--locked`: the scripts add it, and `Cargo.lock` is committed.
  - Every `.rs` file starts with the same nine-line copyright and licence header as `cli/*.cpp`
    (`test/repo.test.ts` checks it, and the same for the `.cpp` and `.hpp` files of
    `parity/oracle/`).
  - A file that is translated from, or takes, code of another project has a dated "Carries
    <project> code" block under the header. The block names the source files, says whose
    copyright they are (the holders) and gives their licence: OrcaSlicer, PrusaSlicer and Slic3r
    (AGPL-3.0), Microsoft STL (Apache-2.0 WITH LLVM-exception), oneTBB (Apache-2.0), Boost and
    Clipper (BSL-1.0), Eigen (MPL-2.0), and so on. Translated code keeps the licence of its source
    and combines with AGPL-3.0-only; `NOTICE` and `licenses/` name each. A test refuses a block
    without a date, a source file, the holders or the licence, and a file that says it is
    translated or ported from other code and has no block.
  - Every sort, selection, heap and float `min` or `max` goes through `orca-stl`, every maths call
    (`sin`, `cos`, `pow`, `exp`, `ln` and the rest, for `f64` and `f32`) through `orca-crt`, and
    no engine crate uses `std`'s `HashMap` or `HashSet`: `clippy.toml` refuses them with
    `disallowed-methods` and `disallowed-types`. `orca-stl` and `orca-crt` allow them only at
    their own definitions, with an `#[allow(clippy::disallowed_methods)]` or
    `#[allow(clippy::disallowed_types)]` on that one item and a comment saying why. Rust's own
    sorts are not the C++ engine's: the order of the comparator calls is part of the result.
  - Use `f32` wherever the C++ uses `float`, and the C++ order of evaluation.
  - A dependency pin (`rayon`, `windows-sys`, `sha2`, the toolchain, MSVC, the SDK) is changed by
    a task of its own, with the probes run again; `Cargo.lock` is committed with it.
  - Update the docs that describe a change with it: this file, `README.md`, `BUILDING.md`, the
    crate's `README.md` and the module's header comment.
- A change is licensed under the licence of the files it changes or adds: `protocol/` under the
  MIT licence (`protocol/LICENSE`), everything else under the GNU Affero General Public License,
  version 3 (`LICENSE`), the licence of OrcaSlicer, which sketch-orca is a modified version of.
