# orca-stl

The standard library algorithms and containers of the C++ engine, with their exact order of operations.

The algorithms and containers of the C++ standard library as Microsoft's implementation runs them: `sort`, `stable_sort`, `nth_element`, `partial_sort`, the heap functions, `min` and `max` with their NaN behaviour, the unordered containers in MSVC's iteration order, and `mt19937` with its distributions.

The order of the comparator calls and the order of the iteration are part of the result of a slice, so they are reproduced, not approximated.

The reference is MSVC 14.42 (Visual Studio 2022 17.12), whose headers come from microsoft/STL at the tag `vs-2022-17.12` (commit 91e425587a0b9b5ac0d1005844c661fee2e2813b). The code it carries is Microsoft's, under Apache-2.0 WITH LLVM-exception (`licenses/Apache-2.0.txt`, `licenses/LLVM-exception.txt`); each module's "Carries Microsoft STL code" block names the files and functions it follows.

- `sort`: `std::sort`, the introsort with its median of three or ninther, fat partition, heap sort fallback and insertion sort of up to 32 elements.
- `stable_sort`: `std::stable_sort` and `std::inplace_merge` with `_Optimistic_temporary_buffer`'s buffer sizes, the chunked merge sort through the buffer and the in-place merges by rotation. While elements sit in the buffer a panicking comparator aborts the process, since the slice would otherwise be left with holes.
- `select`: `std::nth_element`, `std::partial_sort`, `std::lower_bound`, `std::upper_bound`, `std::equal_range`, `std::binary_search` and `std::unique`.
- `heap`: `std::push_heap`, `pop_heap`, `make_heap`, `sort_heap`, `is_heap`, `is_heap_until`, and `std::priority_queue`.
- `minmax`: `std::min`, `std::max` and `std::clamp` with their NaN and signed-zero behaviour, and the element searches. For `float` and `double` with `std::less`, MSVC runs the element searches through `vector_algorithms.cpp`, whose answer with NaNs depends on the SIMD lanes; the functions follow the AVX2 path by default (every x64 processor since 2013, and the oracle's), and `VectorIsa` picks the SSE4.2 or scalar path, so the program gives one answer on every machine.
- `unordered`: `std::hash` (FNV-1a over the bytes) and `std::unordered_map` and `std::unordered_set` in MSVC's list-and-bucket order through inserts, erases, rehashes, copies and assignment. `unordered_multimap` and `unordered_multiset` are not covered.
- `random`: `std::mt19937`, `std::mt19937_64`, `std::seed_seq`, `std::generate_canonical`, `std::uniform_int_distribution`, `std::uniform_real_distribution` and `std::shuffle`, with MSVC's number of engine calls.

`tests/stl.rs` holds every function to what a program built with MSVC 14.42 (`cl /O2 /MT /EHsc /std:c++17`) recorded for the same inputs: the comparator call log and the result of every sort, selection, heap and search, the NaN and zero cases of `min`, `max`, `clamp` and the element searches, `std::hash`, the iteration order and bucket counts of the unordered containers through every operation, and the random number sequences. Each kind of line is compared through the FNV-1a digest of all its lines, and a few lines in full.

`tests/probes.rs` replays every record of the probe program's `stl.sop1` (`parity/SOP1.md`), built with the C++ engine's own compile and link lines: every comparator call of every sort, selection, heap, queue, search and `unique` in order, with the result; `min`, `max`, `clamp` and the element searches; `std::hash`; the unordered containers' scripts; the engines, distributions, `seed_seq` and `shuffle`; the committed subset by default, the full set with `SKETCH_ORCA_PROBES_DEEP` (`crates/orca-probe/README.md`).

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.

## Rules

- Engine crates sort, select, heap and take float minima and maxima only through this crate, and use its hash containers; `clippy.toml` refuses the standard ones.
- The definitions here may name the refused methods and types, each with its own `#[allow]` and a comment.

## Dependencies

None.
