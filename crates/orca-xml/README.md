# orca-xml

XML as expat 2.5.0 parses it, for 3MF files.

The parser the C++ engine's 3MF reader runs: expat 2.5.0 as OrcaSlicer's dependency build
compiles it (`src/deps/EXPAT/expat`, not `deps_src/expat`; MIT), without `XML_DTD`, `XML_NS`,
`XML_CONTEXT_BYTES` or `XML_ATTR_INFO`, with `XML_Char` a `char` holding UTF-8. Translated by the
o2-model task (2026-11-08) as `bbs_3mf.cpp` drives it, so that the reader gets the same callbacks,
split the same way, and the same error with the same line.

## How the reader uses it

- `Parser::new()` is `XML_ParserCreate(NULL)`. A `Handler` holds the three callbacks the reader
  sets (start element, end element, character data); the entity declaration and external entity
  handlers stay unset, as the reader sets them to `NULL`.
- Model files: `Parser::parse` (`XML_Parse`) for each chunk miniz's extraction callback hands over,
  final on the last one. Configuration files: `get_buffer` and `parse_buffer` (`XML_GetBuffer` and
  `XML_ParseBuffer`) with the whole entry, final.
- A handler stops the parse with `Ctl::stop_parser(false)` (`XML_StopParser`), which ends the call
  with "parsing aborted". `Ctl::specified_attribute_count` is `XML_GetSpecifiedAttributeCount`.
- After an error: `error_code`, `error_string` (`XML_ErrorString`), `current_line_number` and
  `current_column_number`, which the reader puts in its messages.

## What is in it

| Module | What it does | C source |
| --- | --- | --- |
| `parse` | The parser: its buffer, the processors (prolog, content, CDATA sections, epilog, error), the XML declaration, the internal subset's general entities, attribute defaults and declarations, start tags with their attributes normalized and defaulted, internal entities expanded in place, positions and stopping | `xmlparse.c` |
| `role` | The prolog's state machine | `xmlrole.c` |
| `tok` | The tokenizer, generic over the encoding as the C includes it three times; the encodings (UTF-8, the internal UTF-8, Latin-1, US-ASCII, UTF-16 in both byte orders) and their conversion to UTF-8; detection from the first bytes and the byte order mark; the XML declaration | `xmltok.c`, `xmltok_impl.inc`, `xmltok_ns.inc` |
| `tables` | Byte types and naming bitmaps, generated from the C's headers | `asciitab.h`, `iasciitab.h`, `latin1tab.h`, `utf8tab.h`, `nametab.h` |
| `error` | `enum XML_Error` and its texts | `expat.h`, `xmlparse.c` |

A pointer is an index into the bytes it points into. When `XML_Parse` returns with its pointers
still in the caller's chunk (after an error, or after a final chunk), the parser keeps a copy of
the bytes the line count will walk, since the chunk is gone by then in Rust. What only a handler
the reader leaves unset would see (comments, processing instructions, declarations) is skipped
where the C skips it. `XML_ResumeParser`, external entities and the other handlers and queries
are not translated (`parity/HAZARDS.md`, orca-xml).

## Probe gate

`tests/probes.rs` replays the records of `xml.sop1` (`parity/SOP1.md`, "Probe files"), which
`bun run engine parity probe --only xml` writes from the C++ engine's own build
(`parity/oracle/probe/xml.cpp`, linked with the dependency build's `expat.lib`): the committed
subset in `parity/probes/` by default, the full set with
`SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes`. Each record holds its inputs and what expat did;
the test does the same and compares the bytes, naming the field of the first difference.

| Record | What it proves |
| --- | --- |
| XPRS | A document fed five ways (`XML_Parse` in chunks with the last final, `XML_GetBuffer` and `XML_ParseBuffer` whole, `XML_Parse` with no chunk final, the same then a final empty call, and the buffer in chunks), the chunks a repeating pattern of sizes, with a stop at a chosen callback and at every later one: each call's status, every callback with its name, data or attributes and the specified count, what each stop returned, then the error code and text, line, column, parsing status and specified count |
| XERR | The error texts of every code and one either side |

The documents: the 3MF reader's own files, each encoding expat knows (UTF-8 with and without a
byte order mark, Latin-1, US-ASCII, UTF-16 in both byte orders, declared and detected, wrong and
unknown), the internal subset (entities with markup, recursion, external and unparsed entities,
attribute defaults and normalization), character references, CDATA sections, line ends, long
names, values and text, deep nesting, every error expat reports for them, seeded mutations of all
of these, and generated model and configuration files up to a few hundred kilobytes fed in
miniz's 32 and 64 KiB chunks. Every record of both sets matched on 2026-11-08 (1196 XPRS
committed, 7755 in the full set).

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only; the code
it carries keeps its own licence, which `NOTICE` names in the crate's own section.

## Dependencies

Declared up front, before the crate is filled (o2-geom, 2026-11-07), so that the tasks that fill the O2 crates in parallel never change `Cargo.toml` or `Cargo.lock`:

- `orca-crt`
- `orca-stl`
- `orca-fmt`
- `orca-probe` (development only: the probe gate)
