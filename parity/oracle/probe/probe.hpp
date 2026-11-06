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
// sketch-orca-probe: what the C++ engine's foundations do, recorded for the Rust translation's
// probe gate (parity/SOP1.md, "Probe files"). This header is shared by the probe sources: the SOP1
// writer, the seeded input generator and the two sets a run writes (the committed subset in the
// repository's parity/probes/ and the full set in <work>/parity/probes/).
//
// Every probe source records its cases into both sets from one generator: the committed set takes
// the first cases of each record, the full set all of them, so the committed set is a true subset.
// Nothing here depends on the machine, the time or the folder: two runs write the same bytes
// (bun scripts/orca.ts parity probe --check).
#pragma once

#include <cstdint>
#include <cstring>
#include <string>
#include <vector>

namespace probe {

// The SOP1 format version these files are written in (parity/SOP1.md).
constexpr uint32_t SOP1_VERSION = 1;

// One record's payload, little-endian, as SOP1.md specifies its values.
struct Payload
{
    std::string bytes;
    Payload &u8(uint8_t v)
    {
        bytes.push_back(char(v));
        return *this;
    }
    Payload &u16(uint16_t v)
    {
        for (int i = 0; i < 2; ++i)
            bytes.push_back(char(uint8_t(v >> (8 * i))));
        return *this;
    }
    Payload &u32(uint32_t v)
    {
        for (int i = 0; i < 4; ++i)
            bytes.push_back(char(uint8_t(v >> (8 * i))));
        return *this;
    }
    Payload &u64(uint64_t v)
    {
        for (int i = 0; i < 8; ++i)
            bytes.push_back(char(uint8_t(v >> (8 * i))));
        return *this;
    }
    Payload &i32(int32_t v) { return u32(uint32_t(v)); }
    Payload &i64(int64_t v) { return u64(uint64_t(v)); }
    Payload &boolean(bool v) { return u8(v ? 1 : 0); }
    Payload &f64(double v)
    {
        uint64_t b;
        std::memcpy(&b, &v, 8);
        return u64(b);
    }
    Payload &f32(float v)
    {
        uint32_t b;
        std::memcpy(&b, &v, 4);
        return u32(b);
    }
    Payload &str(const std::string &s)
    {
        u32(uint32_t(s.size()));
        bytes += s;
        return *this;
    }
    Payload &i32s(const std::vector<int32_t> &v)
    {
        u64(v.size());
        for (int32_t x : v)
            i32(x);
        return *this;
    }
    Payload &i64s(const std::vector<int64_t> &v)
    {
        u64(v.size());
        for (int64_t x : v)
            i64(x);
        return *this;
    }
    Payload &u32s(const std::vector<uint32_t> &v)
    {
        u64(v.size());
        for (uint32_t x : v)
            u32(x);
        return *this;
    }
    Payload &u64s(const std::vector<uint64_t> &v)
    {
        u64(v.size());
        for (uint64_t x : v)
            u64(x);
        return *this;
    }
};

// One SOP1 file, built in memory and written whole.
struct File
{
    std::string bytes;
    uint64_t    records = 0;
    File()
    {
        bytes = "SOP1";
        Payload v;
        v.u32(SOP1_VERSION);
        bytes += v.bytes;
    }
    void add(const char (&kind)[5], const Payload &p)
    {
        bytes.append(kind, 4);
        Payload len;
        len.u64(p.bytes.size());
        bytes += len.bytes;
        bytes += p.bytes;
        ++records;
    }
    // Ends the file with END and writes it; false when it cannot be written.
    bool write(const std::string &path);
};

// The two sets a run writes. `committed` holds the first cases of each record (the repository's
// parity/probes/, under 8 MB with configdef.json), `full` all of them (<work>/parity/probes/).
struct Sets
{
    File committed;
    File full;
    // The number of cases a record of `full_count` cases keeps in the committed set.
    static size_t committed_count(size_t full_count, size_t cap) { return full_count < cap ? full_count : cap; }
};

// What every probe file starts with: HEAD (source "probe", the set's name) and PRUN (the compiler,
// the STL, the UCRT's FMA3 choice at start, which set this is).
void head(Sets &sets, const std::string &name);

// The seeded input generator (splitmix64), the same in every probe source and in the Rust tests.
struct SplitMix
{
    uint64_t state;
    explicit SplitMix(uint64_t seed) : state(seed) {}
    uint64_t next()
    {
        uint64_t z = (state += 0x9E3779B97F4A7C15ull);
        z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ull;
        z = (z ^ (z >> 27)) * 0x94D049BB133111EBull;
        return z ^ (z >> 31);
    }
    uint64_t below(uint64_t n) { return n == 0 ? 0 : next() % n; }
};

inline uint64_t bits(double d)
{
    uint64_t u;
    std::memcpy(&u, &d, 8);
    return u;
}
inline uint32_t bits(float f)
{
    uint32_t u;
    std::memcpy(&u, &f, 4);
    return u;
}
inline double from_bits(uint64_t b)
{
    double d;
    std::memcpy(&d, &b, 8);
    return d;
}
inline float from_bits32(uint32_t b)
{
    float f;
    std::memcpy(&f, &b, 4);
    return f;
}

// The probe sources. Each records into both sets; `crt_symbols` are the CRT maths functions the
// oracle's linker map lists (parity probe reads them from the map).
bool record_crt(Sets &sets, const std::vector<std::string> &crt_symbols, std::string &error);
void record_fmt(Sets &sets);
void record_stl(Sets &sets);
void record_tbb(Sets &sets);
// configdef.json: the engine's ConfigDef and handle_legacy, as JSON text.
std::string record_configdef();

} // namespace probe
