# The crate tests' recording programs

The foundation crates' own tests hold tables and digests of what MSVC 14.42, the static UCRT and
oneTBB 2021.5.0 did, as C++ programs printed them on 2026-11-06. These are those programs, kept
as they were run, so that every recorded value in a test can be traced to the code that printed
it:

| Program | Test | What the test takes from it |
| --- | --- | --- |
| `msvc_crt_fmt.cpp` | `crates/orca-crt/tests/crt.rs`, `crates/orca-fmt/tests/fmt.rs` | `MSVC_DOUBLE_CASTS`, `MSVC_FLOAT_CASTS` and `MSVC_RAND` (crt.rs); `MSVC_OSTREAM` (fmt.rs) |
| `msvc_stl.cpp` | `crates/orca-stl/tests/stl.rs` | the FNV-1a digests and line counts of each kind of line, and the lines it compares in full |
| `tbb.cpp` | `crates/orca-tbb/tests/tbb.rs` | the same, for oneTBB's leaves, reduce trees and orders |

Each file's header comment gives its command line. All were built with `cl.exe` called directly
(no CMake, no IDE: nothing that could start Visual Studio's IDE) in the pinned environment, the one
`scripts/lib/toolchain.ts` finds:

```
"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" 10.0.22000.0 -vcvars_ver=14.42.34433
```

They are records, not part of any build or check. The probe program one folder up
(`bun scripts/orca.ts parity probe`) records these behaviours and more, built against the C++
engine's own build tree, into `parity/probes/` (`parity/SOP1.md`, "Probe files").
