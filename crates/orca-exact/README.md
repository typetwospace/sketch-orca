# orca-exact

Exact arithmetic as Boost 1.84.0 has it: multiprecision's signed-magnitude int128_t and rational.

Exact integer and rational arithmetic as the C++ engine gets it from Boost 1.84.0: Boost.Multiprecision's `int128_t` (a signed-magnitude `cpp_int` of 128 bits, which wraps on overflow otherwise than Rust's `i128`) and `boost::rational`, which libnest2d and `Arrange.cpp` compute with (`<work>/deps/OrcaSlicer_dep/usr/local/include/boost-1_84`). Filled by the o2-arrange task on 2026-11-07.

- `Int128` (`src/int128.rs`) is `cpp_int_backend<128, 128, signed_magnitude, unchecked>` as MSVC builds it, with no `__int128`: four 32-bit limbs and a sign. Addition, subtraction and multiplication wrap the magnitude and keep the sign; division truncates and throws on a zero divisor; a remainder by zero panics, as the C++ faults there; the conversion to `double` rounds as Boost's does.
- `Int128::gcd` is Boost's `eval_gcd` step for step, Lehmer's steps included. Those multiply in 128 bits, so for some operands near 2^127 the products wrap and Boost's bit queries throw `std::domain_error` (or the result is not the gcd); the translation gives the same error or the same value.
- `Rational` (`src/rational.rs`) is `boost::rational<T>` over any `ExactInt`: Boost's normalisation, its gcd-cancelling operators with rationals and integers, its continued-fraction comparison and `rational_cast<double>`.
- Every C++ exception is an `ExactError` whose `what()` is the C++ message: `std::overflow_error` for a division by zero, `bad_rational` for a zero denominator, `std::domain_error` from the gcd.

`parity/HAZARDS.md` (orca-exact) lists the paths where the C++ faults or wraps.

## Probe gate

`tests/probes.rs` replays the XI28 and XRAT records of `arrange.sop1` (`parity/SOP1.md`), which `bun run engine parity probe --only arrange` writes from the C++ engine's own Boost: every `int128_t` operation of 4054 operand pairs and every rational operation of 2507 cases, compared bit for bit, exceptions included. The committed subset in `parity/probes/` runs by default, the full set with `SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes`. `orca-arrange`'s `tests/probes.rs` replays the file's other records and checks that every kind in it has a replay.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only; the code
it carries keeps its own licence, which `NOTICE` names in the crate's own section.

## Dependencies

Declared up front, before the crate is filled (o2-geom, 2026-11-07), so that the tasks that fill the O2 crates in parallel never change `Cargo.toml` or `Cargo.lock`:

- `orca-crt`
- `orca-stl`
- `orca-probe` (development only: the probe gate)
