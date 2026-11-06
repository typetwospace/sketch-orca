// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.
//
// sketch-orca-probe: records what the C++ engine's foundations do (the static UCRT's maths, MSVC
// 14.42's casts, printf, to_chars, iostreams and parsers, the STL's algorithms, containers and
// random numbers, oneTBB 2021.5.0's partitioners, and Orca's ConfigDef), built against the
// oracle's build tree and the engine's dependency build with the engine's flags (CMakeLists.txt),
// so the Rust translation can be held to it (parity/SOP1.md, crates/*/tests/probes.rs). It never
// ships. bun scripts/orca.ts parity probe builds and runs it:
//
//   sketch-orca-probe --committed <folder> --full <folder> --crt-symbols <file>
//
// It writes crt.sop1, fmt.sop1, stl.sop1, tbb.sop1 and configdef.json into both folders: the
// committed subset and the full set (probe.hpp). --crt-symbols names the CRT maths functions of
// the oracle's linker map, one per line. Exit 0 when every file is written, 2 for a command line
// it does not take, 3 when a probe fails.
#include <cstdio>
#include <fstream>
#include <map>
#include <string>
#include <vector>

#include <float.h>

#include "probe.hpp"

namespace probe {

bool File::write(const std::string &path)
{
    Payload end;
    end.u64(records);
    std::string out = bytes;
    out.append("END ", 4);
    Payload len;
    len.u64(end.bytes.size());
    out += len.bytes;
    out += end.bytes;
    std::ofstream f(path, std::ios::binary | std::ios::trunc);
    if (!f)
        return false;
    f.write(out.data(), std::streamsize(out.size()));
    return bool(f);
}

void head(Sets &sets, const std::string &name)
{
    for (int which = 0; which < 2; ++which) {
        File   &f = which == 0 ? sets.committed : sets.full;
        Payload h;
        h.str("probe").str(name).i64(-1).u32(1);
        f.add("HEAD", h);
        Payload r;
        r.u32(uint32_t(_MSC_FULL_VER)).u32(uint32_t(_MSVC_STL_UPDATE)).i32(_get_FMA3_enable()).str(which == 0 ? "committed" : "full");
        f.add("PRUN", r);
    }
}

} // namespace probe

namespace {

int usage(const char *message)
{
    std::fprintf(stderr, "sketch-orca-probe: %s\nusage: sketch-orca-probe --committed <folder> --full <folder> --crt-symbols <file>\n", message);
    return 2;
}

bool write_text(const std::string &path, const std::string &text)
{
    std::ofstream f(path, std::ios::binary | std::ios::trunc);
    if (!f)
        return false;
    f.write(text.data(), std::streamsize(text.size()));
    return bool(f);
}

} // namespace

int main(int argc, char **argv)
{
    std::map<std::string, std::string> opts;
    for (int i = 1; i < argc; ++i) {
        const std::string a = argv[i];
        if (a.rfind("--", 0) != 0 || i + 1 >= argc)
            return usage(("unexpected argument " + a).c_str());
        opts[a.substr(2)] = argv[++i];
    }
    for (const char *need : {"committed", "full", "crt-symbols"})
        if (!opts.count(need))
            return usage((std::string("--") + need + " is missing").c_str());
    if (opts.size() != 3)
        return usage("unknown option");

    std::vector<std::string> symbols;
    {
        std::ifstream in(opts["crt-symbols"]);
        if (!in)
            return usage(("cannot read " + opts["crt-symbols"]).c_str());
        for (std::string line; std::getline(in, line);) {
            while (!line.empty() && (line.back() == '\r' || line.back() == ' '))
                line.pop_back();
            if (!line.empty())
                symbols.push_back(line);
        }
    }

    const std::string committed = opts["committed"], full = opts["full"];
    const auto save = [&](probe::Sets &sets, const char *name) {
        const bool ok = sets.committed.write(committed + "/" + name) && sets.full.write(full + "/" + name);
        if (!ok)
            std::fprintf(stderr, "sketch-orca-probe: cannot write %s\n", name);
        return ok;
    };
    {
        probe::Sets sets;
        std::string error;
        if (!probe::record_crt(sets, symbols, error)) {
            std::fprintf(stderr, "sketch-orca-probe: crt: %s\n", error.c_str());
            return 3;
        }
        if (!save(sets, "crt.sop1"))
            return 3;
    }
    {
        probe::Sets sets;
        probe::record_fmt(sets);
        if (!save(sets, "fmt.sop1"))
            return 3;
    }
    {
        probe::Sets sets;
        probe::record_stl(sets);
        if (!save(sets, "stl.sop1"))
            return 3;
    }
    {
        probe::Sets sets;
        probe::record_tbb(sets);
        if (!save(sets, "tbb.sop1"))
            return 3;
    }
    const std::string config = probe::record_configdef();
    if (!write_text(committed + "/configdef.json", config) || !write_text(full + "/configdef.json", config)) {
        std::fprintf(stderr, "sketch-orca-probe: cannot write configdef.json\n");
        return 3;
    }
    return 0;
}
