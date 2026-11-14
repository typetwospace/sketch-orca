# orca-xml

XML as the C++ engine's 3MF reader parses it: expat 2.5.0, and Boost 1.84's `property_tree` on
rapidxml.

The parser the C++ engine's 3MF reader runs: expat 2.5.0 as OrcaSlicer's dependency build
compiles it (`src/deps/EXPAT/expat`, not `deps_src/expat`; MIT), without `XML_DTD`, `XML_NS`,
`XML_CONTEXT_BYTES` or `XML_ATTR_INFO`, with `XML_Char` a `char` holding UTF-8. Translated by the
o2-model task (2026-11-08) as `bbs_3mf.cpp` drives it, so that the reader gets the same callbacks,
split the same way, and the same error with the same line.

The reader's second parser, for the cut information and the height ranges: Boost 1.84's
`property_tree::read_xml` with flags 0, which runs the rapidxml Boost bundles with
`parse_comment_nodes`, and the tree's `get_child` and `get<T>` (BSL-1.0). Translated by
orca-ptree-port (2026-11-11), so that the reader gets the same tree, the same values and the same
exception texts (see *The property tree* below).

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
| `rapidxml` | rapidxml's parser under `parse_comment_nodes`: the document's nodes, the buffer rewritten in place as rapidxml rewrites it, and `parse_error` with where it stopped | Boost's `property_tree/detail/rapidxml.hpp` |
| `ptree` | `read_xml` with flags 0, the tree, `string_path`, `get_child`, `get<T>` with and without a default, the exceptions, and `stream_translator` over orca-fmt's `num_get` | Boost's `property_tree` (`ptree_implementation.hpp`, `string_path.hpp`, `exceptions.hpp`, `stream_translator.hpp`, `xml_parser_read_rapidxml.hpp`, `file_parser_error.hpp`) |

A pointer is an index into the bytes it points into. When `XML_Parse` returns with its pointers
still in the caller's chunk (after an error, or after a final chunk), the parser keeps a copy of
the bytes the line count will walk, since the chunk is gone by then in Rust. What only a handler
the reader leaves unset would see (comments, processing instructions, declarations) is skipped
where the C skips it. `XML_ResumeParser`, external entities and the other handlers and queries
are not translated (`parity/HAZARDS.md`, orca-xml).

## The property tree

- `read_xml(bytes)` is `read_xml(std::istringstream(bytes), tree)` with flags 0: a `Ptree`, or an
  `XmlParserError` whose `what()` is Boost's `<unspecified file>(<line>): <message>`. The line
  counts the line feeds before where rapidxml stopped, in the buffer as rapidxml has rewritten it
  by then (the NULs it writes after names, and text expanded in place), so it is translated with
  those writes, statement by statement. rapidxml's DOM becomes two arrays, as its memory pool
  holds it: the nodes, the document first, each node's children a list linked by index, and the
  attributes, each node's in a run; names and values are offsets into the buffer. What flags 0
  leaves out is not translated (declaration, doctype and instruction nodes, the closing tag's
  check, trimming and normalising white space).
- A `Ptree` is a node's data and its children in order, each with its key: an element is a child
  keyed by its name, its attributes under `<xmlattr>` first; a comment is a child keyed
  `<xmlcomment>`; text and CDATA join the parent's data.
- `get_child(path)` walks a path split at each `.` (`r.` is `r`, an empty path the node itself),
  taking the first child with an equal key, and throws `No such node (<path>)`;
  `get_child_optional` gives `None` instead. `data()` and `children()` read a node.
- `get::<T>(path)` for `i32` (`int`), `u64` (`size_t`), `f32`, `f64` and `Vec<u8>`
  (`std::string`) reads the data as `stream_translator` does: `>> value` through orca-fmt's
  `istream` (MSVC's `num_get`), then nothing but white space; it throws `conversion of data to
  type "<type>" failed` with Boost's type name. `get_or::<T>(path, default)` is `get` with a
  default and never throws.
- Nothing recurses once per level, where the C++ does (orca-xml-stack, 2026-11-11). rapidxml's
  `parse_element` and `parse_node_contents` call each other for each child element, and Boost's
  `read_xml_node` calls itself; here the parser keeps the open elements on a stack of its own and
  takes a turn of `parse_node_contents` for the innermost one, finishing an element at its closing
  tag as `parse_element` would (the NUL after its name, then `append_node`), so every write and
  error comes in the C++'s order and an error's line is the same. `read_xml` copies the nodes into
  the tree over a stack too, `get_child` walks a path in a loop, and a `Ptree`'s `Clone`, `Drop`,
  `PartialEq` and `Debug` are written by hand over stacks. So the port reads a tree as deep as
  memory allows: a file deep enough to overflow the C++ engine's stack crashes the engine, and the
  port reads it instead (`parity/HAZARDS.md`, orca-model's 3MF table). Tests read, clone, compare,
  drop and replay a document of 1,000,000 levels on a thread with a 256 KiB stack, and two failing
  documents of 1,000,000 levels give the message and line the recursive reader gave.

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

`tests/ptree.rs` replays the records of `ptree.sop1` the same way (`parity/oracle/probe/ptree.cpp`,
built with the dependency build's Boost 1.84 headers; `bun run engine parity probe --only ptree`):

| Record | What it proves |
| --- | --- |
| PTXR | A document through `read_xml` exactly as the reader calls it: the exception's `what()`, or the whole tree in document order (each node's key, data and number of children) |
| PTGT | A document through `read_xml`, then queries on its tree, each on a node reached by child indexes from the root: `get_child` (the node it found, as indexes), `get<T>` for `int`, `size_t`, `float`, `double` and `std::string`, with and without a default, `data()` and the children; each answer, or the `what()` it threw |

The documents: everything rapidxml does under flags 0 (the prolog it skips, comments, CDATA, the
entities and character references at each UTF-8 length and past them, the name characters, white
space, nesting to 200 levels), every `parse_error` flags 0 can reach from each place that throws
it with the line `read_xml` counts in the rewritten buffer, paths (`r.`, `.r`, `.`, empty keys,
repeated keys), 273 number texts in 15 groups under all four types (`DBL_MAX` and the rounding
boundary past it written out in full among them), and the reader's own calls on
3mf-2-numbers, its 25 variants and the other fixtures whose cut information or height ranges reach
`read_xml`; the full set adds seeded mutations of the documents, 2000 seeded number texts and
larger generated documents. Every record of both sets matched on 2026-11-11 (139 PTXR and 60 PTGT
committed, 4249 and 82 in the full set).

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only; the code
it carries keeps its own licence, which `NOTICE` names in the crate's own section.

## Dependencies

Declared up front, before the crate is filled (o2-geom, 2026-11-07), so that the tasks that fill the O2 crates in parallel never change `Cargo.toml` or `Cargo.lock`:

- `orca-crt`
- `orca-stl`
- `orca-fmt`
- `orca-probe` (development only: the probe gate)
