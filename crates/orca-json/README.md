# orca-json

JSON with `nlohmann::json`'s semantics, as the C++ front end uses it (nlohmann/json 3.10.4, the copy OrcaSlicer v2.4.2 carries in `deps_src/nlohmann`), so a file the Rust program reads or writes is read or written as the C++ program does it, byte for byte.

- `Value`: null, a boolean, a signed or an unsigned integer, a double, a string, an array or an object. A parsed number without a `-` is unsigned, one with a `-` is signed, and one with a fraction or an exponent, or too big for 64 bits, is a double, as in nlohmann.
- `Object` and `Order`: an object's members in `json`'s order (`Order::Sorted`, by key, byte for byte, as `std::map<std::string, json>` keeps them) or in `ordered_json`'s (`Order::Insertion`, in the order keys were first set). Setting a key that is there keeps its place; a key given twice in a parsed text keeps its first place and its last value.
- `parse(text, order)`: `json::parse`, with the same refusals and the same `what()` (`Error`): `[json.exception.parse_error.101] parse error at line L, column C: syntax error while parsing value - invalid literal; last read: 'tru'` and the rest, lines and columns included; `out_of_range.406` for a number too big for a double. A byte order mark is skipped, and a NUL byte ends the input, as nlohmann's input adapters do. Numbers are read with orca-fmt's `strtoull`, `strtoll` and `strtod`, the UCRT's.
- `Value::dump(indent)` and `Value::dump_bytes(indent, indent_char, ensure_ascii)`: `dump`, the same bytes. Doubles go through nlohmann's own Grisu2 (`src/dtoa.rs`: the shortest text that reads back, `1e+15` from 1e15 up and `100000000000000.0` below it, `null` for an infinity or a NaN); strings are escaped as `dump_escaped` escapes them, and the indent past 512 characters grows as nlohmann grows it.
- `escape_bytes` and `string_from_bytes`: `dump_escaped` of a `std::string` that need not be UTF-8, with `error_handler_t` strict (`type_error.316`, `invalid UTF-8 byte at index I: 0xBB`), replace or ignore. A `Value`'s strings are Rust strings, so text from outside (an OS message, a file name) goes through `string_from_bytes` with `ErrorHandler::Replace` first; dumping the result writes the bytes `dump(..., error_handler_t::replace)` writes for the raw bytes.

The nlohmann code is under the MIT licence (`licenses/MIT-nlohmann-json.txt`, which also carries the notice of Florian Loitsch's Grisu2 reference implementation); each file that carries it says what it carries in its `Carries` block.

`tests/json.rs` holds the crate to the front end's own files: it parses and dumps again, byte for byte, the configdef probe (indent 1), `parity/goldens/front/config-def.json` (indent 2), `version.json` (compact) and every intake result; and it checks the key orders of both kinds of object, duplicate keys, number dumps at the edges of the fixed and exponent forms, pretty layout, escapes under each error handler, 43 parse errors with nlohmann's exact messages, `out_of_range.406`, and the byte order mark and NUL handling.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only.

## Rules

- Every JSON file the Rust program reads or writes goes through this crate; `serde_json` orders, reads and writes numbers differently.
- Code translated from nlohmann keeps its structure and names, function for function (`scan_string`, `scan_number`, `dump_escaped`, `grisu2`), so that it can be compared with 3.10.4.

## Dependencies

- `orca-fmt` (the number parsers)
- `orca-stl` (`lower_bound`, the search of a sorted object)
