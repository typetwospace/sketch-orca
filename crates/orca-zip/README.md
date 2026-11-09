# orca-zip

ZIP archives as miniz reads them, for 3MF files.

The read path of miniz 2.1.0 (`deps_src/miniz`, `MZ_VERSION` "10.1.0", MIT) that OrcaSlicer's 3MF
reader takes through `miniz_extension.cpp`, translated by the o2-model task (2026-11-07/08), bit
for bit. The writer (`mz_zip_writer_*`, `tdefl`) is not here: the command line reads 3MF files and
writes G-code.

## What is in it

| Module | What it does | C source |
| --- | --- | --- |
| `reader` | `mz_zip_archive`: an archive opened from a file, from memory or through a read function; the central directory read and checked (ZIP64 records and extra fields, multi-disk numbers, the flags `bbs_3mf` passes), the names sorted for a search without case; `locate_file`, `file_stat`, `get_extra` (Bambu Lab's addition), and an entry extracted to a buffer, through a callback or to a file, with its CRC-32 checked. | `miniz.c`, `miniz.h` |
| `tinfl` | `tinfl_decompress`, the inflater: a coroutine that stops where its input runs out or its output is full, with the C's state numbers, 64-bit bit buffer, read-ahead and eight-byte match copies. | `miniz.c` |
| `crc` | `mz_crc32`, with the C's table. | `miniz.c` |
| `error` | `mz_zip_error`, `mz_zip_get_error_string` and `MZ_Archive::get_errorstr`'s texts. | `miniz.c`, `miniz_extension.cpp` |
| `extension` | `open_zip_reader` (the UTF-8 path opened as `boost::nowide::fopen` opens it), `close_zip_reader` and `MZ_Archive`. | `miniz_extension.cpp` |

The C's slips are kept, each with the error it leaves in `m_last_error` (the module headers name
them): the "stored" check that reads the method and the file time as one word, a ZIP64 extra field
past the directory read from the first record's place, the end-of-directory search that walks to a
negative offset in an archive of 4097 to 8188 bytes, and a callback extraction that reports a CRC
mismatch as a decompression failure. `m_time` is left out (`parity/HAZARDS.md`, orca-zip).

## Probe gate

`tests/probes.rs` replays the records of `zip.sop1` (`parity/SOP1.md`, "Probe files"), which
`bun run engine parity probe --only zip` writes from the C++ engine's own build
(`parity/oracle/probe/zip.cpp`): the committed subset in `parity/probes/` by default, the full set
with `SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes`. Each record holds its inputs and what miniz
computed; the test computes the same and compares the bytes, naming the field of the first
difference.

| Record | What it proves |
| --- | --- |
| ZCRC | `mz_crc32` from a running value |
| ZINF | `tinfl_decompress` driven call by call: seeded input and output chunks, a buffer that does not wrap or a ring (rings smaller than the window too), each call's status and counts, the output and the Adler-32 |
| ZARC | An archive opened three ways (and a missing path and a folder): its entries' statistics, extra fields and data extracted four ways, names located with and without case and path, and the archive closed twice |
| ZERR | The error texts of every code and two outside |

Every record of both sets matched on 2026-11-08. The full set found two bugs the committed set had
missed: the eight-byte match copy in a ring smaller than the distance, and a read of no bytes at an
offset past the end of an archive in memory.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only; the code
it carries keeps its own licence, which `NOTICE` names in the crate's own section.

## Dependencies

Declared up front, before the crate is filled (o2-geom, 2026-11-07), so that the tasks that fill the O2 crates in parallel never change `Cargo.toml` or `Cargo.lock`:

- `orca-crt`
- `orca-stl`
- `orca-probe` (development only: the probe gate)
