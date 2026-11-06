# orca-fmt

printf, to_chars, iostream double output and strtod, stod and from_chars parsing, as the C++ engine gets them.

Number formatting and parsing that gives the bytes the C++ engine's runtime gives (MSVC 14.42's standard library and the static UCRT, in the "C" locale):

- `printf`: `sprintf`, `snprintf` and `sprintf_s` (`_snprintf_s`), written from the C standard (`d i u x X o c s e E f F g G a A %`, every flag, width, precision and `*`, and the UCRT's length modifiers) with the UCRT's choices: its spelling of infinities and NaNs (`inf`, `nan(ind)`, `nan(snan)`), exact decimal expansion at every precision, and its two error behaviours (the standard family prints an invalid conversion as text; the secure one fails).
- `to_chars`: `std::to_chars`, translated from microsoft/STL: Ryu for the shortest round-trip form, Ryu printf for the precision forms, and the hexadecimal forms; integers in every base.
- `ostream`: `std::ostream << double` (and `float`, widened) as MSVC's `num_put` builds it: the printf format from the stream's flags, then the locale's decimal point, grouping and fill; and `std::to_string(double)`.
- `parse`: `std::from_chars`, translated from microsoft/STL, for integers and floating values; the UCRT's `strtod`, `strtof`, `atof`, `strtol`, `strtoul`, `strtoll`, `strtoull`, `atoi`, `atol` and `atoll` around it, with the three places where the UCRT's rounding differs from the STL's (digits after the 768th dropped, an exact tie rounded up after some divisions, the exponent capped at 5201); and the STL's `std::stod`, `stof`, `stoi`, `stol`, `stoul`, `stoll` and `stoull`.

The Microsoft STL code is under Apache-2.0 WITH LLVM-exception (`licenses/Apache-2.0.txt` and `licenses/LLVM-exception.txt`); each file that carries it says what it carries and from where in its `Carries` block.

`tests/fmt.rs` holds the crate to the UCRT linked into the test program: printf against `__stdio_common_vsprintf` and `__stdio_common_vsnprintf_s` on millions of seeded values and formats, the parsers against the UCRT's own functions (exact ties and long inputs among the texts), `ostream` against the UCRT's `sprintf_s` of MSVC's format, and round trips of every form; and to 25 streams that a program built with MSVC 14.42 printed. The crate's `to_chars`, `from_chars`, `sto*`, `ostream` and `to_string` were also compared with such a program on 300,000 seeded cases of each kind, with no difference.

`tests/probes.rs` replays every record of the probe program's `fmt.sop1` (`parity/SOP1.md`): `snprintf`'s text and count, `to_chars` with its `errc`, `ostream` output under every flag, width, fill and locale, `to_string`, and the parsers' values, lengths and statuses, byte for byte; the committed subset by default, the full set with `SKETCH_ORCA_PROBES_DEEP` (`crates/orca-probe/README.md`).

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.

## Rules

- Engine crates format and parse numbers only through this crate; Rust's `format!("{}")` of a float is not `printf`, and `str::parse` is not `strtod`.
- Code translated from microsoft/STL keeps the STL's structure and names, function for function, so that it can be compared with the tag it came from (vs-2022-17.12).

## Dependencies

None.
