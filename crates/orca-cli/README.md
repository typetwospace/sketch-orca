# orca-cli

The Rust program, sketch-orca-rs: sketch-orca's commands, intake, declines and crash result.

The command line of the Rust translation: the program `sketch-orca-rs`, a separate program from the C++ `sketch-orca` of `cli/`, under the same licence. It runs the protocol's commands (`version`, `config-def`, `run`) with the intake the C++ front end has, declines a job it cannot do yet before it writes any output (result code `not-ported`, exit -8, with a list of what is missing), and on a crash writes the `internal-error` result and exits with -1.

For now it prints the usage and exits as the C++ program does with no command (exit -2); the commands arrive with the tasks that follow.

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
