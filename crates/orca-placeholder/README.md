# orca-placeholder

Orca's placeholder parser: the conditions of presets and of custom G-code.

Orca's placeholder parser (`PlaceholderParser.cpp`, a Boost.Spirit grammar in the C++): the expressions of the presets' compatibility conditions (`compatible_printers_condition`, `compatible_prints_condition`) that the presets stage evaluates. Phase O4's `orca-gcode` extends it to the custom G-code templates.

## What it does

The o2-front task of phase O2 filled it (2026-11-07). `evaluate_boolean_expression(template, config, override)` is `PlaceholderParser::evaluate_boolean_expression`: a result, or the text of the `PlaceholderParserError` the C++ code throws, word for word, which the presets stage prints or logs (`orca-cli`'s `presets.rs`).

- `parser.rs`: the grammar as Spirit runs it: white space skipped, backtracking, the `>` expectations and where their errors point, the variables (the override first, then the configuration; a percentage through its `ratio_over` chain), `process_error_message` with its tag table, and the error's line passed through `boost::nowide`'s UTF-8 round trip, as the C++ code passes it (a byte that is not UTF-8 becomes U+FFFD).
- `expr.rs`: the values (bool, `int`, `double`, string) and their operators, with the C++ code's type errors, `int` wrap-around and `digits()`.
- `number.rs`: Spirit's `strict_double` and `int_`.
- `regex.rs`: `boost::regex` and `regex_match` for `=~`, `!~` and `one_of`, as an MSVC build parses and matches with its narrow `w32_regex_traits`: literals, `.`, sets with ranges and classes, groups, alternation, the repeats, greedy and lazy, `^` and `$`, the escapes and one-letter classes the traits know (`\d \w \s \l \u \h \v` and their negations), and the error messages and positions of a pattern Boost refuses.

What it does not take, it declines (`Error::Unsupported`) rather than guesses, and the job is declined in the presets stage: a line width (`Flow::extrusion_width`), an error at the C++ code's null iterator, a regex form past those above (assertions such as `\b`, back references, perl extensions other than `(?:`, possessive repeats, named classes such as `[[:alpha:]]`, hexadecimal, octal and named character escapes), and a class tested against a byte past ASCII, which the Windows code page decides (`parity/HAZARDS.md`, orca-placeholder).

## Probe gate

`tests/probes.rs` replays the records of `placeholder.sop1` (`parity/SOP1.md`, "Probe files": PHCF and PHEV), which `bun run engine parity probe --only placeholder` writes from the C++ engine's own build: the committed subset in `parity/probes/` by default (about 370 hand-written expressions, each without and with a printer's override), the full set with `SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes` (those and every compatibility condition of the vendor profiles against each printer or process of its vendor: 3260 evaluations). Each evaluation's outcome must be the crate's: the same result, the same error text, and a decline exactly where the C++ code threw another exception.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only; the code
it carries keeps its own licence, which `NOTICE` names in the crate's own section.

## Dependencies

Declared up front, before the crate is filled (o2-geom, 2026-11-07), so that the tasks that fill the O2 crates in parallel never change `Cargo.toml` or `Cargo.lock`:

- `orca-crt`
- `orca-stl`
- `orca-fmt`
- `orca-config`
- `orca-probe` (development only: the probe gate)
