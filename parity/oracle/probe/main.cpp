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
// The probe programs: they record what the C++ engine's foundations and libraries do (the static
// UCRT's maths, MSVC 14.42's casts, printf, to_chars, iostreams and parsers, the STL's algorithms,
// containers and random numbers, oneTBB 2021.5.0's partitioners, Orca's ConfigDef, and from phase
// O2 the geometry and the rest of what the model's reading reaches), built against the oracle's
// build tree and the engine's dependency build with the engine's flags (CMakeLists.txt), so the
// Rust translation can be held to it (parity/SOP1.md, crates/*/tests/probes.rs). They never ship.
//
// Each probe of the registry (scripts/lib/parity/probe.ts, PROBE_REGISTRY) is a program of its
// own, sketch-orca-probe-<name>, built from this file and the probe's source, which defines
// probe::target() (probe.hpp). bun scripts/orca.ts parity probe [--only <name>] builds and runs
// them:
//
//   sketch-orca-probe-<name> --committed <folder> --full <folder> [--crt-symbols <file>]
//
// Each writes its one file into both folders: the committed subset and the full set (probe.hpp).
// --crt-symbols, which only the crt probe takes and needs, names the CRT maths functions of the
// oracle's linker map, one per line. Exit 0 when the file is written, 2 for a command line it does
// not take, 3 when the probe fails. Before 2026-11-07 one program, sketch-orca-probe, wrote the
// first five files.
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

bool save(Sets &sets, const Options &options, const std::string &file, std::string &error)
{
    if (sets.committed.write(options.committed + "/" + file) && sets.full.write(options.full + "/" + file))
        return true;
    error = "cannot write " + file;
    return false;
}

namespace {

bool write_text(const std::string &path, const std::string &text)
{
    std::ofstream f(path, std::ios::binary | std::ios::trunc);
    if (!f)
        return false;
    f.write(text.data(), std::streamsize(text.size()));
    return bool(f);
}

} // namespace

bool save_text(const std::string &text, const Options &options, const std::string &file, std::string &error)
{
    if (write_text(options.committed + "/" + file, text) && write_text(options.full + "/" + file, text))
        return true;
    error = "cannot write " + file;
    return false;
}

bool write_stub(const std::string &name, const Options &options, std::string &error)
{
    Sets sets;
    head(sets, name);
    return save(sets, options, name + ".sop1", error);
}

} // namespace probe

namespace {

int usage(const probe::Target &t, const std::string &message)
{
    std::fprintf(stderr, "sketch-orca-probe-%s: %s\nusage: sketch-orca-probe-%s --committed <folder> --full <folder>%s\n", t.name, message.c_str(), t.name,
                 t.crt_symbols ? " --crt-symbols <file>" : "");
    return 2;
}

} // namespace

int main(int argc, char **argv)
{
    const probe::Target &t = probe::target();
    std::map<std::string, std::string> opts;
    for (int i = 1; i < argc; ++i) {
        const std::string a = argv[i];
        if (a.rfind("--", 0) != 0 || i + 1 >= argc)
            return usage(t, "unexpected argument " + a);
        opts[a.substr(2)] = argv[++i];
    }
    std::vector<std::string> need = {"committed", "full"};
    if (t.crt_symbols)
        need.push_back("crt-symbols");
    for (const std::string &n : need)
        if (!opts.count(n))
            return usage(t, "--" + n + " is missing");
    if (opts.size() != need.size())
        return usage(t, "unknown option");

    probe::Options options;
    options.committed = opts["committed"];
    options.full      = opts["full"];
    if (t.crt_symbols) {
        std::ifstream in(opts["crt-symbols"]);
        if (!in)
            return usage(t, "cannot read " + opts["crt-symbols"]);
        for (std::string line; std::getline(in, line);) {
            while (!line.empty() && (line.back() == '\r' || line.back() == ' '))
                line.pop_back();
            if (!line.empty())
                options.crt_symbols.push_back(line);
        }
    }

    std::string error;
    if (!t.run(options, error)) {
        std::fprintf(stderr, "sketch-orca-probe-%s: %s\n", t.name, error.c_str());
        return 3;
    }
    return 0;
}
