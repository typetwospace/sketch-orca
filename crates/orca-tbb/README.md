# orca-tbb

oneTBB 2021.5.0's concurrency and partitioners as fixed split trees, run on rayon.

oneTBB 2021.5.0's `max_concurrency` and its partitioners, kept as fixed split trees and join orders, and run on rayon 1.12.0 (Windows: `windows-sys` for the processor count, with processor groups). The split tree depends on the concurrency P and on nothing else, so a result never depends on how threads are scheduled.

The C++ engine's oneTBB splits a loop further whenever a thread steals work, so its leaves change from run to run; the parity oracle pins them by running the engine with one thread in an arena of concurrency P. Each partitioner here splits as that one thread does, and the leaves then run either on a rayon pool (`Arena::new(P)`) or, task for task in oneTBB's order, on the calling thread (`Arena::serial(P)`, the oracle's run). Outside any arena the algorithms run on rayon's global pool at the default concurrency. The code it carries is Intel's oneTBB 2021.5.0 (the tag `v2021.5.0`), under Apache-2.0 (`licenses/Apache-2.0.txt`); each module's "Carries Intel oneTBB code" block names the files and functions it follows.

- `concurrency`: `hardware_concurrency` as oneTBB reads it on Windows (the process affinity mask, or every active processor of every processor group when the mask is full), `default_concurrency`, `max_concurrency`, and `Arena` (`task_arena`) with oneTBB's 4 MiB worker stacks. A serial arena keeps oneTBB's last-in-first-out task pool: a wait runs the newest task, whatever group it belongs to, until its own group is done.
- `range`: `BlockedRange` (`blocked_range` over `i32`, `u32`, `i64`, `u64`, `isize` and `usize`, with the C++ arithmetic of each type, the even split and the proportional split in `float`) and `BlockedRange2d`, and the `Range` trait for other ranges.
- `partitioner`: `Partitioner::Auto`, `Simple` and `Static` (and `DeterministicPartitioner`) as the split decisions of one thread: the auto partitioner's 2P initial chunks, depth budget of 5 and range pool of 8, the static partitioner's proportional split into P parts, and `leaves`, which lists a range's leaves in the order one thread runs them.
- `parallel_for`: `parallel_for` over a range, `parallel_for_index` and `parallel_for_index_step` (the integer forms, `(last - first - 1) / step + 1` iterations), and `parallel_for_each` and `parallel_for_each_mut` over slices.
- `parallel_reduce`: `parallel_reduce` (on one thread a single body folds every leaf left to right, which is also the result in a parallel arena, where the leaves then run one after another), `parallel_deterministic_reduce` (a body split at every split and joined in the split tree's order, run in parallel with the same result), their functional forms, and the `ReduceBody` trait.
- `invoke`: `parallel_invoke` (and the `parallel_invoke!` macro) in oneTBB's recursive order, and `task_group` with `run`, `run_and_wait`, `wait`, `cancel` and `is_canceling`.

A panic in a task is oneTBB's exception: the first one cancels the task's group and the groups started inside its tasks, the tasks that have not started are skipped, and the wait resumes the panic. Two cases where oneTBB never finishes or splits a running body panic instead: a task that waits for its own task group, and, in a serial arena, a reduction leaf that waits for outside tasks and so lets a part of the same reduction start beside it (oneTBB would split the running body, which Rust's borrowing forbids). oneTBB's `global_control`, `affinity_partitioner`, `parallel_for_each` with a feeder and over non-random-access iterators, `blocked_range3d` and `parallel_pipeline` are not translated.

`tests/tbb.rs` holds the crate to what a program built with MSVC 14.42 and linked with the engine's static oneTBB 2021.5.0 recorded with one thread in arenas of concurrency 1 to 64: the leaves of every partitioner over `long long`, `size_t`, `int` (past `INT_MAX` included) and two-dimensional ranges, the strings of every split and join of `parallel_reduce` and `parallel_deterministic_reduce`, the order of the integer forms and of `parallel_for_each`, and the order of `parallel_invoke` and of task groups with nested tasks and loops, 13,520 lines in all, compared through the FNV-1a digest of each kind of line and a few lines in full. It also checks that a parallel arena runs the same leaves once each and gives the same reductions, and how panics and cancellation travel.

`tests/probes.rs` replays every record of the probe program's `tbb.sop1` (`parity/SOP1.md`), recorded with the engine's oneTBB as the oracle runs it, in `Arena::serial` arenas of each recorded P: `max_concurrency()`, the leaves of every loop in the order they ran, the reduction trees, and the order of the integer loops, `parallel_for_each`, `parallel_invoke` and task groups; the committed subset by default, the full set with `SKETCH_ORCA_PROBES_DEEP` (`crates/orca-probe/README.md`).

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.

## Rules

- A parallel loop in an engine crate is written against this crate, so it splits the way TBB splits it at the same P.

## Dependencies

- `rayon`
- `windows-sys`
