# Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
# SPDX-License-Identifier: AGPL-3.0-only
#
# sketch-orca-probe: the program that records what the C++ engine's foundations do (main.cpp), built
# against the parity oracle's build tree with the engine's own compile and link lines. Never run
# CMake on this by hand: bun scripts/orca.ts parity probe (scripts/lib/parity/probe.ts) reads the
# lines from the oracle's build.ninja, writes them as response files, and configures this with
# Ninja and cl, with no Fortran compiler to look for, into <work>/oracle/probe-build:
#
#   PROBE_COMPILE_RSP  the defines, include folders and flags of the engine's cli/config_def.cpp
#                      (so libslic3r's headers, nlohmann/json.hpp and legacy_keys.inc are found),
#                      without the debug-information and parallel-build flags;
#   PROBE_LINK_RSP     the engine's link flags and libraries (libslic3r.lib, tbb12.lib and the
#                      rest), with the oracle's compiled cli/gui_symbols.cpp, which libslic3r needs.
#
# Nothing here starts Visual Studio's IDE: the generator is Ninja, the only language CXX.

cmake_minimum_required(VERSION 3.20)
cmake_policy(SET CMP0091 NEW)
project(sketch_orca_probe LANGUAGES CXX)

foreach(var PROBE_COMPILE_RSP PROBE_LINK_RSP)
    if(NOT DEFINED ${var} OR NOT EXISTS "${${var}}")
        message(FATAL_ERROR "${var} must name an existing response file; run bun scripts/orca.ts parity probe")
    endif()
endforeach()

# The engine's flags come whole from the response files, so none of CMake's own.
set(CMAKE_CXX_FLAGS "")
set(CMAKE_CXX_FLAGS_RELEASE "")
set(CMAKE_MSVC_RUNTIME_LIBRARY MultiThreaded)

add_executable(sketch-orca-probe main.cpp crt.cpp fmt.cpp stl.cpp tbb.cpp configdef.cpp)
target_compile_options(sketch-orca-probe PRIVATE "@${PROBE_COMPILE_RSP}")
target_link_options(sketch-orca-probe PRIVATE "@${PROBE_LINK_RSP}")
