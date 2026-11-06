# orca-cli

The Rust program, sketch-orca-rs: sketch-orca's commands, intake, declines and crash result.

The command line of the Rust translation: the program `sketch-orca-rs`, a separate program from the C++ `sketch-orca` of `cli/`, under the same licence. It is to run the protocol's commands (`version`, `config-def`, `run`) with the intake the C++ front end has, decline a job it cannot do yet before it writes any output (result code `not-ported`, exit -8, with a list of what is missing), and on a crash write the `internal-error` result and exit with -1.

What it answers so far:

- `help`, `--help`, `-h`: `cli/main.cpp`'s usage text on stdout, exit 0. No command: the usage on stderr after `sketch-orca: no command`, exit -2; an unknown command, an option a command does not take, a missing or repeated option or a missing value: the same message `cli/main.cpp` gives, the usage, exit -2.
- `version` (and `--version`): the C++ program's `version.json` line (`engine`, `engineVersion` from the workspace's version, `orcaVersion` 2.4.2, `orcaTag` v2.4.2, `orcaCommit` 8500fcdccaa10b5099ac20d252af3a7c560046f1), plus `implementation: "rust"` (`src/version.rs`).
- `config-def --out <file>`: the same file as the C++ program's, byte for byte (`parity/goldens/front/config-def.json`, with the engine version of the build): every setting of orca-config's definitions but SLA's, `coNone`'s and the obsolete keys, then the obsolete keys (`src/config_def.rs`). It is written as `cli/common.cpp`'s `write_file_atomic` writes it (`<file>.tmp`, then a rename; `src/files.rs`), with the same messages and exit -13 when it cannot be.
- `run --job <file> --out <file> [--progress <file>]`: its command line is checked as `cli/main.cpp` checks it, and a command line it does not take still gets an `invalid-params` result at `--out` when one can be written there. The run follows `cli/run.cpp`'s `run_job` up to the job's work: the job is read and checked as `cli/job.cpp` reads it, in the same order and with the same messages (`src/job.rs`); the run's files (the job, every absolute path it names, the result, the progress file and the outputs with the files they are written through) are told apart as the C++ tells them apart (`src/result.rs`, `src/paths.rs`: Boost.Filesystem's `is_absolute`, and two names of one file on Windows); the result and the progress file are written as the C++ writes them (`src/result.rs`). Every job the C++ refuses gets the same result, exit code and progress file (`parity/goldens/front/intake/`).
- The decline (`src/decline.rs`): a valid job is checked against the families `parity/parity.json` certifies. One that reaches a family this program does not do (`decline::PORTED` and certified), which for now is every `info`, `layers` and `slice` job, gets result code `not-ported`, exit -8 and `missing`, the families in parity.json's order (`protocol/examples/result-not-ported.json`), before any output is written. parity.json is read at build time (`include_str!`).
- A crash (`src/crash.rs`): an unhandled exception, an `abort` or a panic writes the `internal-error` result the C++ writes ("the engine crashed (<what>) during '<stage>', in Orca's code"), and the process ends with -1; no system crash dialog is shown. `SKETCH_ORCA_TEST_CRASH=<stage>` crashes with an access violation when the run reaches that stage, `<stage>:abort` with an abort, as in the C++ program; `<stage>:panic` (this program only) with a panic; the point `decline` (this program only) is just before a decline is written, since this program reaches no stage past `job` yet.

On Windows stdout and stderr end their lines with a carriage return and a line feed, as the C++ program's text-mode streams do; files are written as bytes.

The modules: `main.rs` (the command line), `common.rs` (the failures and their exit codes, `cli/common.hpp`), `files.rs` (reading files, and writing them whole through `<file>.tmp`), `paths.rs`, `job.rs`, `result.rs` (results, progress, the run's files and `run`), `decline.rs`, `crash.rs`, `version.rs` and `config_def.rs`.

`tests/version.rs`, `tests/config_def.rs`, `tests/intake.rs`, `tests/decline.rs` and `tests/crash.rs` run the built program hidden (no console window) and compare what it writes with the front end's goldens and `protocol/examples/` (`tests/common/mod.rs` is what they share); the modules hold their unit tests. `bun scripts/orca.ts parity certify` runs the program on the whole corpus (the repository's `README.md`).

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.

Build it with `bun run rs:build`; the program is `target/debug/sketch-orca-rs.exe` (`bun scripts/rust.ts build --release` builds `target/release/sketch-orca-rs.exe`).

Carries Boost.Filesystem code in `src/paths.rs` (Boost Software License 1.0; the repository's `NOTICE`).

## Rules

- The usage text, the exit codes and the result files follow `cli/` and `protocol/`; a difference is a bug in one of them.

## Dependencies

- `orca-crt`
- `orca-fmt`
- `orca-stl`
- `orca-tbb`
- `orca-probe`
- `orca-json`
- `orca-config`
- `sha2`
- `windows-sys`
