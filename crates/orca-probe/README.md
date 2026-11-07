# orca-probe

Reads the SOP1 probe files the oracle records, for the probe gate.

SOP1 (`parity/SOP1.md`) is the container of the oracle's dumps and of the probe programs' records (`parity/oracle/probe/`, one program per probe file, run by `bun scripts/orca.ts parity probe` or, for one, `parity probe --only <name>`): the magic `SOP1`, a `u32` version, then records of a four-letter kind and a payload, ending with `END `. `parse` and `read_file` refuse anything else, as the harness's reader does (`scripts/lib/parity/neutral.ts`); a `Reader` walks a payload's little-endian values, strings and vectors, and `finish` refuses a payload with bytes left over.

`load_probe(name)` reads `<folder>/<name>.sop1` and checks its HEAD (`probe`, the file's name) and its PRUN (the set): the committed subset in the repository's `parity/probes/` by default, or the full set from the folder `SKETCH_ORCA_PROBES_DEEP` names (`<work>/parity/probes`). A committed file found where the full set is asked for, or the other way round, is an error, so a deep run never passes on the small set. `Mismatches` counts the comparisons of a test and lists the first that differ, so a failing probe test shows its cases together.

The probe gate is the crates' `tests/probes.rs`, which replay every record of their file with this crate:

| Test | File | Records |
| --- | --- | --- |
| `crates/orca-crt/tests/probes.rs` | `crt.sop1` | MATH (with FMA3 on and off), MATS, CAST, RAND, QSRT (every comparator call of `qsort`) |
| `crates/orca-fmt/tests/probes.rs` | `fmt.sop1` | PRTF, TOCH, OSTR, TOST, PARS |
| `crates/orca-stl/tests/probes.rs` | `stl.sop1` | SORT (every comparator call), VALS, HASH, UMAP, USET, RNGS, SSEQ, SHUF |
| `crates/orca-tbb/tests/probes.rs` | `tbb.sop1` | TBBA, TBBF, TBBR, TBBO, in serial arenas of each recorded P |
| `crates/orca-eigen/tests/probes.rs`, `crates/orca-geom/tests/probes.rs` | `geom.sop1` | none yet: a stub until o2-geom records there |
| `crates/orca-qhull/tests/probes.rs` | `qhull.sop1` | none yet: a stub until o2-mesh records there |
| `crates/orca-mesh/tests/probes.rs` | `mesh.sop1`, `cstdio.sop1` | none yet: stubs until o2-mesh records there |
| `crates/orca-zip/tests/probes.rs`, `crates/orca-xml/tests/probes.rs`, `crates/orca-model/tests/probes.rs` | `zip.sop1`, `xml.sop1`, `model.sop1` | none yet: stubs until o2-model records there |
| `crates/orca-exact/tests/probes.rs`, `crates/orca-arrange/tests/probes.rs` | `arrange.sop1` | none yet: a stub until o2-arrange records there |
| `crates/orca-placeholder/tests/probes.rs` | `placeholder.sop1` | none yet: a stub until o2-front records there |

Phase O2's files (2026-11-07) are stubs, HEAD and PRUN only, until the task that fills each records there; its crates' probe tests fail as soon as the file holds a record they do not replay, so no record goes unreplayed. The orca-probe unit test reads the HEAD and PRUN of every committed file.

```
bun run rs:test -p orca-crt -p orca-fmt -p orca-stl -p orca-tbb
SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes bun run rs:test -p orca-crt -p orca-fmt -p orca-stl -p orca-tbb
bun run rs:test:release -p orca-crt -p orca-fmt -p orca-stl -p orca-tbb --test probes
SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes bun run rs:test:release -p orca-crt -p orca-fmt -p orca-stl -p orca-tbb --test probes
```

The gate runs in both profiles. `rs:test` builds the tests as the `dev` profile does;
`rs:test:release` (`cargo test --locked --release`) builds them with the release profile the
shipped program is built with (`Cargo.toml`: `opt-level = 3`, fat LTO, one codegen unit), so
inlining or constant folding there cannot change a maths call, a float operation or a CRT result
unseen.

Every record of both sets passed on 2026-11-06. One record depends on the machine that made it: TBBA `outside` is oneTBB's `max_concurrency()` outside any arena, which is the recording machine's processor count, so the test compares it only on a machine with as many processors and otherwise checks that `max_concurrency()` there is `default_concurrency()` (`parity/HAZARDS.md`, "Threads").

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.

## Dependencies

None. It is a development dependency of `orca-crt`, `orca-fmt`, `orca-stl` and `orca-tbb`, of phase O2's ten crates, and a dependency of `orca-cli`.
