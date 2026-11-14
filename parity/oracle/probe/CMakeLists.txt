# Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
# SPDX-License-Identifier: AGPL-3.0-only
#
# The probe programs: one per probe file of the registry (scripts/lib/parity/probe.ts,
# PROBE_REGISTRY), each recording what one part of the C++ engine does (main.cpp), built against
# the parity oracle's build tree with the engine's own compile and link lines. Never run CMake on
# this by hand: bun scripts/orca.ts parity probe [--only <name>] (scripts/lib/parity/probe.ts)
# reads the lines from the oracle's build.ninja, writes them as response files, and configures this
# with Ninja and cl, with no Fortran compiler to look for, into <work>/oracle/probe-build:
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

# The registry's names, in its order (test/parity-probe.test.ts holds the two lists equal; added
# 2026-11-07). Each is a program sketch-orca-probe-<name> of main.cpp, which every program shares,
# and <name>.cpp, which defines probe::target(), with the .cpp files of a folder <name>/ beside it
# if the probe has one. So a task that fills a probe changes only its own sources. Phase O3's nine
# probes (layer to fill) were appended as stubs on 2026-11-13 (orca-o3-skeleton).
set(PROBE_NAMES crt fmt stl tbb configdef geom qhull mesh cstdio zip xml ptree model arrange placeholder layer index paths slice voronoi arachne walls surfaces fill)

add_library(sketch-orca-probe-main OBJECT main.cpp)
target_compile_options(sketch-orca-probe-main PRIVATE "@${PROBE_COMPILE_RSP}")

foreach(name IN LISTS PROBE_NAMES)
    file(GLOB extra CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/${name}/*.cpp")
    add_executable(sketch-orca-probe-${name} ${name}.cpp ${extra} $<TARGET_OBJECTS:sketch-orca-probe-main>)
    target_compile_options(sketch-orca-probe-${name} PRIVATE "@${PROBE_COMPILE_RSP}")
    target_link_options(sketch-orca-probe-${name} PRIVATE "@${PROBE_LINK_RSP}")
endforeach()
