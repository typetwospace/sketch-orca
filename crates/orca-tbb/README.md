# orca-tbb

oneTBB 2021.5.0's concurrency and partitioners as fixed split trees, run on rayon.

oneTBB 2021.5.0's `max_concurrency` and its partitioners, kept as fixed split trees and join orders, and run on rayon 1.12.0 (Windows: `windows-sys` for the processor count, with processor groups). The split tree depends on the concurrency P and on nothing else, so a result never depends on how threads are scheduled.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.

## Rules

- A parallel loop in an engine crate is written against this crate, so it splits the way TBB splits it at the same P.

## Dependencies

- `rayon`
- `windows-sys`
