# orca-stl

The standard library algorithms and containers of the C++ engine, with their exact order of operations.

The algorithms and containers of the C++ standard library as Microsoft's implementation runs them: `sort`, `stable_sort`, `nth_element`, `partial_sort`, the heap functions, `min` and `max` with their NaN behaviour, the unordered containers in MSVC's iteration order, and `mt19937` with its distributions.

The order of the comparator calls and the order of the iteration are part of the result of a slice, so they are reproduced, not approximated.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.

## Rules

- Engine crates sort, select, heap and take float minima and maxima only through this crate, and use its hash containers; `clippy.toml` refuses the standard ones.
- The definitions here may name the refused methods and types, each with its own `#[allow]` and a comment.

## Dependencies

None.
