# orca-crt

The C runtime the C++ engine links, called as it is: maths, float-to-integer conversions, rand.

The home of every call into the static UCRT that the C++ engine makes: one `#[inline(never)]` wrapper per maths function (`sin`, `cos`, `pow`, `exp`, and the rest, for `double` and for `float`), MSVC's float-to-integer conversions, and `rand` and `srand` with the UCRT's per-thread state.

The crate is `#![no_builtins]`, so the compiler cannot replace a call with its own implementation or fold it into a constant: the answer is the one the C++ program gets from the same `libucrt.lib`. The maths itself is never translated, because the UCRT's source is not open; the wrappers call into the library the Rust program links statically (`.cargo/config.toml`).

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.

## Rules

- Engine crates call maths only through this crate (`clippy.toml` refuses `f64::sin` and the rest).
- A wrapper is allowed to name the Rust method it replaces only on its own item, with `#[allow(clippy::disallowed_methods)]` and a comment.

## Dependencies

None.
