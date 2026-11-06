# orca-cli

The Rust program, sketch-orca-rs: sketch-orca's commands, intake, declines and crash result.

The command line of the Rust translation: the program `sketch-orca-rs`, a separate program from the C++ `sketch-orca` of `cli/`, under the same licence. It is to run the protocol's commands (`version`, `config-def`, `run`) with the intake the C++ front end has, decline a job it cannot do yet before it writes any output (result code `not-ported`, exit -8, with a list of what is missing), and on a crash write the `internal-error` result and exit with -1.

What it answers so far:

- `help`, `--help`, `-h`: `cli/main.cpp`'s usage text on stdout, exit 0. No command: the usage on stderr after `sketch-orca: no command`, exit -2; an unknown command, an option a command does not take, a missing or repeated option or a missing value: the same message `cli/main.cpp` gives, the usage, exit -2.
- `version` (and `--version`): the C++ program's `version.json` line (`engine`, `engineVersion` from the workspace's version, `orcaVersion` 2.4.2, `orcaTag` v2.4.2, `orcaCommit` 8500fcdccaa10b5099ac20d252af3a7c560046f1), plus `implementation: "rust"` (`src/version.rs`).
- `config-def --out <file>`: the same file as the C++ program's, byte for byte (`parity/goldens/front/config-def.json`, with the engine version of the build): every setting of orca-config's definitions but SLA's, `coNone`'s and the obsolete keys, then the obsolete keys (`src/config_def.rs`). It is written as `cli/common.cpp`'s `write_file_atomic` writes it (`<file>.tmp`, then a rename; `src/files.rs`), with the same messages and exit -13 when it cannot be.
- `run --job <file> --out <file> [--progress <file>]`: its command line is checked as `cli/main.cpp` checks it; a run it takes is declined with exit -8 until the intake (task O1.9) arrives.

On Windows stdout and stderr end their lines with a carriage return and a line feed, as the C++ program's text-mode streams do; files are written as bytes.

`tests/version.rs` and `tests/config_def.rs` run the built program hidden (no console window) and compare what it writes with the front end's goldens; `src/main.rs` holds the unit tests of the usage, the exit codes and the refusals.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.

Build and run it with `bun run rs:build`; the program is `target/release/sketch-orca-rs.exe` (or `target/debug/` for a debug build).

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
