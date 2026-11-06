# orca-crt

The C runtime the C++ engine links, called as it is: maths, float-to-integer conversions, rand.

The home of every call into the static UCRT that the C++ engine makes:

- `math`: one `#[inline(never)]` wrapper per maths function the engine calls (`sin`, `cos`, `pow`, `exp`, and the rest, for `double` and for `float`), and `set_fma3_enable` and `get_fma3_enable`, which pass `_set_FMA3_enable` and `_get_FMA3_enable` through. `sqrt`, `sqrtf`, `fabs`, `fabsf` and `copysignf` stay the IEEE operations MSVC compiles them to (`sqrtsd`, `sqrtss`, sign-bit operations).
- `convert`: MSVC 14.42's float-to-integer casts for every integer width: `cvttsd2si`'s indefinite value for NaN and out-of-range input, the low bits for the narrow types, and the unsigned 64-bit sequence.
- `rand`: `rand` and `srand`, the UCRT's linear congruential generator with one state per thread, starting at 1.

The crate is `#![no_builtins]`, so the compiler cannot replace a call with its own implementation or fold it into a constant: the answer is the one the C++ program gets from the same `libucrt.lib`. The maths itself is never translated, because the UCRT's source is not open; the wrappers call into the library the Rust program links statically (`.cargo/config.toml`).

`tests/crt.rs` holds each maths wrapper to the UCRT function called through a function pointer hidden by `black_box`, bit for bit, on special and seeded random arguments with FMA3 off and on; the IEEE operations to the instructions; the conversions to `cvttsd2si`, `cvttss2si` and MSVC's unsigned sequence run as instructions, and to the values a program built with MSVC 14.42 printed for the edge cases; and `rand` to the UCRT's on fresh threads and to that program's sequences.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.

## Rules

- Engine crates call maths only through this crate (`clippy.toml` refuses `f64::sin` and the rest), and cast floats to integers through `convert` where the C++ casts them.
- A wrapper is allowed to name the Rust method it replaces only on its own item, with `#[allow(clippy::disallowed_methods)]` and a comment.

## Dependencies

None.
