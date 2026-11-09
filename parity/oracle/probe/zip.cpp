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
// zip.sop1: miniz's read path as the 3MF reader takes it through miniz_extension.cpp
// (parity/SOP1.md, "Probe files"), recorded by the o2-model task (2026-11-07); orca-zip's
// tests/probes.rs replays it. Each record holds its inputs, then what miniz computed from them:
//
//   ZCRC  mz_crc32 of bytes from a running value.
//   ZINF  tinfl_decompress driven call by call over a deflate or zlib stream: the input given in
//         seeded chunks, the output into a buffer that does not wrap (in seeded chunks too) or
//         into a ring, as the zip reader's two extractions use it; each call's status and what it
//         took and gave, the output and the Adler-32.
//   ZARC  an archive opened from a file (open_zip_reader), from memory (mz_zip_reader_init_mem)
//         or through a read function (mz_zip_reader_init): the error and its texts, then each
//         entry's mz_zip_reader_file_stat, mz_zip_reader_get_extra and its data extracted with
//         mz_zip_reader_extract_to_mem, mz_zip_reader_extract_to_callback (a callback that
//         refuses one chunk, now and then) and mz_zip_reader_extract_to_file, names found with
//         mz_zip_reader_locate_file and extracted with mz_zip_reader_extract_file_to_mem, and
//         the archive closed twice.
//   ZERR  mz_zip_get_error_string and MZ_Archive::get_errorstr of every code and two outside.
//
// The streams come from miniz's own compressor at its levels and strategies, made by hand, cut
// short, with junk after them or with bits flipped. The archives come from miniz's writer and
// from a builder here that writes each field as told (ZIP64 records and extra fields, multi-disk
// numbers, flags, methods, lies in the sizes and the CRC), cut short or with bytes changed. No
// value depends on the time or the folder: the writer is given a fixed time, and the temporary
// folder's name is not recorded. The committed set takes the small cases.
#include "probe.hpp"

#include <algorithm>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <stdexcept>
#include <string>
#include <vector>

#include <process.h>

#include <libslic3r/miniz_extension.hpp>

namespace probe {

namespace {

using namespace Slic3r;

uint64_t fnv1a(const void *data, size_t n)
{
    uint64_t             h = 0xcbf29ce484222325ull;
    const unsigned char *p = static_cast<const unsigned char *>(data);
    for (size_t i = 0; i < n; ++i) {
        h ^= p[i];
        h *= 0x100000001b3ull;
    }
    return h;
}
uint64_t fnv1a(const std::string &s) { return fnv1a(s.data(), s.size()); }

void add(Sets &sets, const char (&kind)[5], const Payload &p, bool committed)
{
    if (committed)
        sets.committed.add(kind, p);
    sets.full.add(kind, p);
}

std::string random_bytes(SplitMix &g, size_t n)
{
    std::string s(n, '\0');
    for (char &c : s)
        c = char(g.next());
    return s;
}

// Text like a 3MF model: vertices and triangles with seeded numbers.
std::string model_text(SplitMix &g, size_t vertices)
{
    std::string s = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"millimeter\">\n <resources>\n  <object id=\"1\">\n   <mesh>\n    <vertices>\n";
    char        buf[160];
    for (size_t i = 0; i < vertices; ++i) {
        std::snprintf(buf, sizeof(buf), "     <vertex x=\"%d.%03d\" y=\"%d.%03d\" z=\"%d\"/>\n", int(g.below(250)), int(g.below(1000)), int(g.below(250)),
                      int(g.below(1000)), int(g.below(60)));
        s += buf;
    }
    s += "    </vertices>\n    <triangles>\n";
    for (size_t i = 0; i + 2 < vertices; i += 2) {
        std::snprintf(buf, sizeof(buf), "     <triangle v1=\"%d\" v2=\"%d\" v3=\"%d\"/>\n", int(i), int(i + 1), int(i + 2));
        s += buf;
    }
    s += "    </triangles>\n   </mesh>\n  </object>\n </resources>\n</model>\n";
    return s;
}

std::string from_heap(void *out, size_t out_len)
{
    if (!out)
        return std::string();
    std::string s(static_cast<const char *>(out), out_len);
    mz_free(out);
    return s;
}

std::string deflate_raw(const std::string &src, int level, int strategy = 0, int window_bits = -15)
{
    size_t        out_len = 0;
    const mz_uint flags   = tdefl_create_comp_flags_from_zip_params(level, window_bits, strategy);
    void         *out     = tdefl_compress_mem_to_heap(src.data(), src.size(), &out_len, int(flags));
    return from_heap(out, out_len);
}

std::string deflate_flags(const std::string &src, int flags)
{
    size_t out_len = 0;
    void  *out     = tdefl_compress_mem_to_heap(src.data(), src.size(), &out_len, flags);
    return from_heap(out, out_len);
}

// ---- ZCRC ------------------------------------------------------------------------------------

void record_crc(Sets &sets)
{
    SplitMix g(0x2C3C0001u);
    for (int i = 0; i < 48; ++i) {
        size_t      n   = i < 10 ? size_t(i) : size_t(g.below(i < 40 ? 300 : 70000));
        std::string s   = i == 9 ? std::string("123456789") : random_bytes(g, n);
        uint32_t    crc = i % 3 == 0 ? 0u : uint32_t(g.next());
        Payload     p;
        p.u32(crc).str(s).u32(uint32_t(mz_crc32(crc, reinterpret_cast<const mz_uint8 *>(s.data()), s.size())));
        add(sets, "ZCRC", p, s.size() <= 300);
    }
}

// ---- ZINF ------------------------------------------------------------------------------------

struct InfSchedule
{
    uint8_t  ring;        // 1: a ring of `cap` bytes, the output going to its end each call
    uint64_t cap;         // the ring's size, or the size of the buffer that does not wrap
    uint32_t flags;       // TINFL_FLAG_PARSE_ZLIB_HEADER, TINFL_FLAG_COMPUTE_ADLER32
    uint8_t  more_always; // TINFL_FLAG_HAS_MORE_INPUT on every call
    uint64_t seed;
    uint32_t max_in;      // 0: all the input left
    uint32_t max_out;     // 0: all the room left (not in a ring)
};

void record_inf_one(Sets &sets, const std::string &stream, const InfSchedule &s, bool committed)
{
    Payload p;
    p.u8(s.ring).u64(s.cap).u32(s.flags).u8(s.more_always).u64(s.seed).u32(s.max_in).u32(s.max_out).str(stream);

    // tinfl_init sets the state alone; the rest is zeroed so that the Adler-32 read after a
    // refused call (TINFL_STATUS_BAD_PARAM, which sets nothing) is not what the stack held.
    tinfl_decompressor r;
    std::memset(&r, 0, sizeof(r));
    tinfl_init(&r);
    SplitMix             g(s.seed);
    std::vector<uint8_t> out(size_t(s.cap), 0);
    std::string          produced;
    Payload              log, first;
    uint64_t             calls = 0, in_pos = 0, out_pos = 0;
    tinfl_status         st    = TINFL_STATUS_FAILED;
    const uint64_t       size  = stream.size();
    for (;;) {
        const uint64_t in_avail = s.max_in ? std::min<uint64_t>(1 + g.below(s.max_in), size - in_pos) : size - in_pos;
        uint64_t       next, out_size;
        if (s.ring) {
            next     = out_pos % s.cap;
            out_size = s.cap - next;
        } else {
            next     = out_pos;
            out_size = s.max_out ? std::min<uint64_t>(1 + g.below(s.max_out), s.cap - out_pos) : s.cap - out_pos;
        }
        size_t          in_sz = size_t(in_avail), out_sz = size_t(out_size);
        const mz_uint32 fl    = s.flags | (s.ring ? 0u : uint32_t(TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF)) |
                             ((in_pos + in_avail < size || s.more_always) ? uint32_t(TINFL_FLAG_HAS_MORE_INPUT) : 0u);
        st = tinfl_decompress(&r, reinterpret_cast<const mz_uint8 *>(stream.data()) + in_pos, &in_sz, out.data(), out.data() + next, &out_sz, fl);
        Payload entry;
        entry.i32(int32_t(st)).u64(in_sz).u64(out_sz);
        log.bytes += entry.bytes;
        if (calls < 16)
            first.bytes += entry.bytes;
        produced.append(reinterpret_cast<const char *>(out.data()) + next, out_sz);
        in_pos += in_sz;
        out_pos += out_sz;
        ++calls;
        if (st == TINFL_STATUS_NEEDS_MORE_INPUT && in_pos == size)
            break;
        if (st != TINFL_STATUS_NEEDS_MORE_INPUT && st != TINFL_STATUS_HAS_MORE_OUTPUT)
            break;
        if (!s.ring && out_pos == s.cap && st == TINFL_STATUS_HAS_MORE_OUTPUT && in_sz == 0 && out_sz == 0)
            break;
        if (calls >= 400000)
            break;
    }
    p.u64(calls);
    p.bytes += first.bytes;
    p.u64(fnv1a(log.bytes)).i32(int32_t(st)).u64(in_pos).u64(out_pos).u32(tinfl_get_adler32(&r)).u64(fnv1a(produced));
    p.boolean(produced.size() <= 256);
    if (produced.size() <= 256)
        p.str(produced);
    add(sets, "ZINF", p, committed);
}

std::string flip_bits(SplitMix &g, std::string s, int n)
{
    for (int i = 0; i < n && !s.empty(); ++i) {
        const size_t at = size_t(g.below(s.size()));
        s[at]           = char(s[at] ^ (1 << g.below(8)));
    }
    return s;
}

void record_inf(Sets &sets)
{
    SplitMix g(0x2C3C0002u);
    struct Source
    {
        std::string name, bytes;
    };
    std::vector<Source> sources;
    sources.push_back({"empty", ""});
    sources.push_back({"a", "a"});
    sources.push_back({"hello", "Hello, hello, hello, hello world! Hello, hello, hello world!"});
    sources.push_back({"model small", model_text(g, 6)});
    sources.push_back({"random 300", random_bytes(g, 300)});
    sources.push_back({"model", model_text(g, 600)});
    sources.push_back({"random 70000", random_bytes(g, 70000)});
    sources.push_back({"run", std::string(100000, 'A')});
    {
        std::string mixed = random_bytes(g, 40000);
        for (int k = 0; k < 40; ++k) {
            const size_t from = size_t(g.below(mixed.size() - 300));
            mixed += mixed.substr(from, 1 + size_t(g.below(300)));
            mixed += random_bytes(g, size_t(g.below(50)));
        }
        sources.push_back({"far matches", mixed});
    }

    const std::vector<InfSchedule> schedules = {
        {0, 0, 0, 0, 1, 0, 0},        // all at once, a buffer that does not wrap
        {0, 0, 0, 0, 2, 97, 300},     // seeded chunks
        {1, 32768, 0, 0, 3, 0, 0},    // a ring, all the input at once
        {1, 32768, 0, 0, 4, 4000, 0}, // a ring, seeded input
        {0, 0, 0, 0, 5, 1, 0},        // a byte at a time
        {0, 0, 0, 1, 6, 0, 0},        // more input promised to the end
        {1, 1024, 0, 0, 7, 0, 0},     // a ring smaller than the window
        {1, 1000, 0, 0, 8, 0, 0},     // a ring not a power of two
    };
    // commit: 0 the full set alone, 1 schedules 0, 1, 2 and 6 in the committed set too, 2 also 7.
    auto run = [&](const std::string &stream, size_t src_size, bool zlib, int commit) {
        for (size_t k = 0; k < schedules.size(); ++k) {
            InfSchedule s = schedules[k];
            if (k == 4 && stream.size() > 20000)
                continue; // a byte at a time over a long stream
            if (!s.ring)
                s.cap = src_size + 256;
            s.flags = zlib ? uint32_t(TINFL_FLAG_PARSE_ZLIB_HEADER) : (k % 2 ? uint32_t(TINFL_FLAG_COMPUTE_ADLER32) : 0u);
            const bool committed = (commit >= 1 && (k == 0 || k == 1 || k == 2 || k == 6)) || (commit >= 2 && k == 7);
            record_inf_one(sets, stream, s, committed);
        }
    };
    struct Comp
    {
        int level, strategy, window_bits;
    };
    const Comp comps[] = {{6, 0, -15}, {6, 0, 15}, {1, 0, -15}, {9, 0, 15}, {10, 0, -15}, {6, 2, -15}, {6, 3, -15}, {6, 4, -15}, {0, 0, -15}};
    for (const Source &src : sources) {
        const bool small = src.bytes.size() <= 1200;
        for (size_t ci = 0; ci < std::size(comps); ++ci) {
            const Comp &c = comps[ci];
            if (!small && c.level != 6 && c.level != 0)
                continue;
            const bool commit = small && (ci == 0 || ci == 1 || ci == 5 || ci == 8);
            run(deflate_raw(src.bytes, c.level, c.strategy, c.window_bits), src.bytes.size(), c.window_bits > 0, commit ? 1 : 0);
        }
    }
    // Raw blocks only, and static blocks only (with a zlib header).
    for (const Source &src : sources) {
        if (src.bytes.size() > 70000)
            continue;
        const int commit = src.bytes.size() <= 300 ? 1 : 0;
        run(deflate_flags(src.bytes, TDEFL_DEFAULT_MAX_PROBES | TDEFL_FORCE_ALL_RAW_BLOCKS), src.bytes.size(), false, commit);
        run(deflate_flags(src.bytes, TDEFL_DEFAULT_MAX_PROBES | TDEFL_FORCE_ALL_STATIC_BLOCKS | TDEFL_WRITE_ZLIB_HEADER), src.bytes.size(), true, commit);
    }
    // Made by hand.
    const std::string hand[] = {
        std::string("\x01\x05\x00\xFA\xFFhello", 10),         // a stored block
        std::string("\x01\x05\x00\xFA\xFEhello", 10),         // its length's complement wrong
        std::string("\x07", 1),                               // block type 3
        std::string("\x03\x00", 2),                           // an empty static block
        std::string("\x00\x05\x00\xFA\xFFhello\x03\x00", 12), // a stored block, then the last
        std::string("\x78\x9C\x03\x00\x00\x00\x00\x01", 8),   // zlib, empty
        std::string("\x78\x9D\x03\x00\x00\x00\x00\x01", 8),   // its header check wrong
        std::string("\x88\x98\x03\x00\x00\x00\x00\x01", 8),   // a 64 KiB window
        std::string("\x78\xBB\x03\x00\x00\x00\x00\x01", 8),   // a preset dictionary
        std::string("\x78\x9C\x03\x00\x00\x00\x00\x02", 8),   // the Adler-32 wrong
        std::string(),
    };
    for (const std::string &h : hand) {
        const bool zlib = h.size() >= 2 && (h[0] == '\x78' || h[0] == '\x88');
        run(h, 64, zlib, 2);
    }
    // Random bits after a dynamic block's header.
    for (int i = 0; i < 24; ++i) {
        std::string s = random_bytes(g, 4 + size_t(g.below(i < 12 ? 40 : 400)));
        s[0]          = char((s[0] & ~7) | 5);
        run(s, 600, false, i < 12 ? 1 : 0);
    }
    // Cut short, junk after, bits flipped.
    const std::string bases[] = {deflate_raw(sources[3].bytes, 6), deflate_raw(sources[3].bytes, 6, 0, 15), deflate_raw(sources[5].bytes, 6),
                                 deflate_raw(sources[8].bytes, 6)};
    const size_t      sizes[] = {sources[3].bytes.size(), sources[3].bytes.size(), sources[5].bytes.size(), sources[8].bytes.size()};
    for (int b = 0; b < 4; ++b) {
        const std::string &base = bases[b];
        for (int i = 0; i < 6; ++i)
            run(base.substr(0, size_t(g.below(base.size()))), sizes[b], b == 1, b == 0 ? 1 : 0);
        run(base + random_bytes(g, 9), sizes[b], b == 1, b < 2 ? 1 : 0);
        for (int i = 0; i < 10; ++i)
            run(flip_bits(g, base, 1 + int(g.below(3))), sizes[b], b == 1, b == 0 && i < 4 ? 1 : 0);
    }
}

// ---- Archives --------------------------------------------------------------------------------

void put16(std::string &s, uint32_t v)
{
    s.push_back(char(v & 0xFF));
    s.push_back(char((v >> 8) & 0xFF));
}
void put32(std::string &s, uint32_t v)
{
    put16(s, v & 0xFFFF);
    put16(s, v >> 16);
}
void put64(std::string &s, uint64_t v)
{
    put32(s, uint32_t(v));
    put32(s, uint32_t(v >> 32));
}
void patch16(std::string &s, size_t at, uint32_t v)
{
    s[at]     = char(v & 0xFF);
    s[at + 1] = char((v >> 8) & 0xFF);
}
void patch32(std::string &s, size_t at, uint32_t v)
{
    patch16(s, at, v & 0xFFFF);
    patch16(s, at + 2, v >> 16);
}

struct RawEntry
{
    std::string name;
    std::string data; // the bytes after the local header
    uint16_t    method = 0, flags = 0, time = 0x6000, date = 0x5761, made_by = 20, needed = 20, disk = 0, int_attr = 0;
    uint32_t    crc = 0, ext_attr = 0;
    uint64_t    comp = 0, uncomp = 0;
    bool        zip64_sizes = false; // 0xFFFFFFFF sizes and offset in the central record, the real ones in a ZIP64 extra field
    std::string extra_central, extra_local, comment;
};

RawEntry stored(const std::string &name, const std::string &src)
{
    RawEntry e;
    e.name   = name;
    e.data   = src;
    e.crc    = uint32_t(mz_crc32(0, reinterpret_cast<const mz_uint8 *>(src.data()), src.size()));
    e.comp   = src.size();
    e.uncomp = src.size();
    return e;
}
RawEntry deflated(const std::string &name, const std::string &src, int level = 6)
{
    RawEntry e = stored(name, src);
    e.data     = deflate_raw(src, level);
    e.method   = 8;
    e.comp     = e.data.size();
    return e;
}

struct RawZip
{
    std::string           prefix;
    std::vector<RawEntry> entries;
    bool                  zip64           = false;
    uint32_t              z64_total_disks = 1;
    uint64_t              z64_record_size = 44;
    std::string           comment;
};

struct Built
{
    std::string         bytes;
    std::vector<size_t> records; // where each central record starts
    std::vector<size_t> locals;  // where each local header starts
    size_t              cdir_ofs = 0, eocd = 0;
};

Built build(const RawZip &z)
{
    Built b;
    b.bytes = z.prefix;
    std::vector<uint64_t> local_ofs;
    for (const RawEntry &e : z.entries) {
        local_ofs.push_back(b.bytes.size() - z.prefix.size());
        b.locals.push_back(b.bytes.size());
        std::string &s = b.bytes;
        put32(s, 0x04034b50);
        put16(s, e.needed);
        put16(s, e.flags);
        put16(s, e.method);
        put16(s, e.time);
        put16(s, e.date);
        put32(s, e.crc);
        put32(s, uint32_t(e.comp));
        put32(s, uint32_t(e.uncomp));
        put16(s, uint32_t(e.name.size()));
        put16(s, uint32_t(e.extra_local.size()));
        s += e.name;
        s += e.extra_local;
        s += e.data;
    }
    const size_t cdir_start = b.bytes.size();
    b.cdir_ofs              = cdir_start;
    for (size_t i = 0; i < z.entries.size(); ++i) {
        const RawEntry &e = z.entries[i];
        std::string    &s = b.bytes;
        b.records.push_back(s.size());
        std::string extra = e.extra_central;
        if (e.zip64_sizes) {
            std::string f;
            put16(f, 1);
            put16(f, 24);
            put64(f, e.uncomp);
            put64(f, e.comp);
            put64(f, local_ofs[i]);
            extra = f + extra;
        }
        put32(s, 0x02014b50);
        put16(s, e.made_by);
        put16(s, e.needed);
        put16(s, e.flags);
        put16(s, e.method);
        put16(s, e.time);
        put16(s, e.date);
        put32(s, e.crc);
        put32(s, e.zip64_sizes ? 0xFFFFFFFFu : uint32_t(e.comp));
        put32(s, e.zip64_sizes ? 0xFFFFFFFFu : uint32_t(e.uncomp));
        put16(s, uint32_t(e.name.size()));
        put16(s, uint32_t(extra.size()));
        put16(s, uint32_t(e.comment.size()));
        put16(s, e.disk);
        put16(s, e.int_attr);
        put32(s, e.ext_attr);
        put32(s, e.zip64_sizes ? 0xFFFFFFFFu : uint32_t(local_ofs[i]));
        s += e.name;
        s += extra;
        s += e.comment;
    }
    const uint64_t cdir_size = b.bytes.size() - cdir_start;
    const uint64_t cdir_ofs  = cdir_start - z.prefix.size();
    if (z.zip64) {
        const uint64_t z64_ofs = b.bytes.size() - z.prefix.size();
        std::string   &s       = b.bytes;
        put32(s, 0x06064b50);
        put64(s, z.z64_record_size);
        put16(s, 45);
        put16(s, 45);
        put32(s, 0);
        put32(s, 0);
        put64(s, z.entries.size());
        put64(s, z.entries.size());
        put64(s, cdir_size);
        put64(s, cdir_ofs);
        put32(s, 0x07064b50);
        put32(s, 0);
        put64(s, z64_ofs);
        put32(s, z.z64_total_disks);
    }
    b.eocd         = b.bytes.size();
    std::string &s = b.bytes;
    put32(s, 0x06054b50);
    put16(s, 0);
    put16(s, 0);
    put16(s, z.zip64 ? 0xFFFFu : uint32_t(z.entries.size()));
    put16(s, z.zip64 ? 0xFFFFu : uint32_t(z.entries.size()));
    put32(s, z.zip64 ? 0xFFFFFFFFu : uint32_t(cdir_size));
    put32(s, z.zip64 ? 0xFFFFFFFFu : uint32_t(cdir_ofs));
    put16(s, uint32_t(z.comment.size()));
    s += z.comment;
    return b;
}

struct WEntry
{
    std::string name, data, comment, extra_central;
    int         level = 6;
};

// An archive as miniz's writer makes it, at a fixed time.
std::string write_zip(const std::vector<WEntry> &entries, mz_uint init_flags = 0)
{
    mz_zip_archive z;
    mz_zip_zero_struct(&z);
    if (!mz_zip_writer_init_heap_v2(&z, 0, 0, init_flags))
        throw std::runtime_error("mz_zip_writer_init_heap_v2 failed");
    MZ_TIME_T t = 1700000000;
    for (const WEntry &e : entries) {
        if (!mz_zip_writer_add_mem_ex_v2(&z, e.name.c_str(), e.data.data(), e.data.size(), e.comment.empty() ? nullptr : e.comment.data(),
                                         mz_uint16(e.comment.size()), mz_uint(e.level), 0, 0, &t, nullptr, 0,
                                         e.extra_central.empty() ? nullptr : e.extra_central.data(), mz_uint(e.extra_central.size()))) {
            mz_zip_writer_end(&z);
            throw std::runtime_error("mz_zip_writer_add_mem_ex_v2 failed: " + e.name);
        }
    }
    void  *buf  = nullptr;
    size_t size = 0;
    if (!mz_zip_writer_finalize_heap_archive(&z, &buf, &size)) {
        mz_zip_writer_end(&z);
        throw std::runtime_error("mz_zip_writer_finalize_heap_archive failed");
    }
    std::string out = from_heap(buf, size);
    mz_zip_writer_end(&z);
    return out;
}

struct Query
{
    std::string name;
    uint32_t    flags       = 0;
    bool        has_comment = false;
    std::string comment;
};

struct ArchiveCase
{
    std::string        label;
    std::string        bytes;
    uint32_t           init_flags     = 0;
    uint32_t           extra_buf_size = 1024;
    uint32_t           fail_at        = 0xFFFFFFFFu;
    std::vector<Query> queries;
    int                commit = 0; // 0 none, 1 from memory, 2 from a file and from memory, 3 all three
};

void put_stat(Payload &p, const mz_zip_archive_file_stat &s)
{
    p.u32(s.m_file_index).u64(s.m_central_dir_ofs).u16(s.m_version_made_by).u16(s.m_version_needed).u16(s.m_bit_flag).u16(s.m_method).u32(s.m_crc32);
    p.u64(s.m_comp_size).u64(s.m_uncomp_size).u16(s.m_internal_attr).u32(s.m_external_attr).u64(s.m_local_header_ofs).u32(s.m_comment_size);
    p.boolean(s.m_is_directory != 0).boolean(s.m_is_encrypted != 0).boolean(s.m_is_supported != 0).i32(s.m_is_utf8);
    p.str(std::string(s.m_filename)).str(std::string(s.m_comment));
}

struct UserSource
{
    const std::string *bytes;
};
size_t user_read(void *opaque, mz_uint64 file_ofs, void *buf, size_t n)
{
    const std::string &s = *static_cast<UserSource *>(opaque)->bytes;
    if (file_ofs >= s.size())
        return 0;
    const size_t k = size_t(std::min<uint64_t>(s.size() - file_ofs, n));
    std::memcpy(buf, s.data() + file_ofs, k);
    return k;
}

struct CallbackLog
{
    uint64_t    calls   = 0;
    uint32_t    fail_at = 0xFFFFFFFFu;
    Payload     log, first;
    std::string data;
};
size_t callback(void *opaque, mz_uint64 ofs, const void *buf, size_t n)
{
    CallbackLog &c = *static_cast<CallbackLog *>(opaque);
    Payload      e;
    e.u64(ofs).u64(n);
    c.log.bytes += e.bytes;
    if (c.calls < 6)
        c.first.bytes += e.bytes;
    c.data.append(static_cast<const char *>(buf), n);
    const bool fail = c.calls == c.fail_at;
    ++c.calls;
    return fail ? n - 1 : n;
}

constexpr uint64_t MAX_EXTRACT = 8u << 20;

// how: 0 a file (open_zip_reader), 1 memory, 2 a read function, 3 a path that does not exist,
// 4 a folder.
void record_arc_one(Sets &sets, const std::filesystem::path &dir, const ArchiveCase &c, uint8_t how, bool committed)
{
    Payload p;
    p.str(c.label).str(c.bytes).u8(how).u32(c.init_flags).u32(c.extra_buf_size).u32(c.fail_at);
    p.u64(c.queries.size());
    for (const Query &q : c.queries) {
        p.str(q.name).u32(q.flags).boolean(q.has_comment);
        if (q.has_comment)
            p.str(q.comment);
    }

    const std::filesystem::path archive_path = dir / "a.zip";
    const std::filesystem::path out_path     = dir / "x.bin";
    std::error_code             ec;
    if (how == 0) {
        std::ofstream f(archive_path, std::ios::binary | std::ios::trunc);
        f.write(c.bytes.data(), std::streamsize(c.bytes.size()));
        if (!f)
            throw std::runtime_error("cannot write the archive");
    }
    MZ_Archive      arc;
    mz_zip_archive &z = arc.arch;
    UserSource      src{&c.bytes};
    bool            ok = false;
    switch (how) {
    case 0: ok = open_zip_reader(&z, archive_path.string()); break;
    case 1: ok = mz_zip_reader_init_mem(&z, c.bytes.data(), c.bytes.size(), c.init_flags); break;
    case 2:
        z.m_pRead      = user_read;
        z.m_pIO_opaque = &src;
        ok             = mz_zip_reader_init(&z, c.bytes.size(), c.init_flags);
        break;
    case 3: ok = open_zip_reader(&z, (dir / "missing.zip").string()); break;
    default: ok = open_zip_reader(&z, dir.string()); break;
    }
    p.boolean(ok).i32(int32_t(z.m_last_error)).str(arc.get_errorstr()).str(mz_zip_get_error_string(z.m_last_error));
    p.u32(mz_zip_reader_get_num_files(&z)).u64(z.m_archive_size).u64(z.m_central_directory_file_ofs).boolean(mz_zip_is_zip64(&z) != 0);

    const uint32_t n = ok ? std::min<uint32_t>(mz_zip_reader_get_num_files(&z), 40) : 0;
    for (uint32_t i = 0; i <= n; ++i) {
        mz_zip_archive_file_stat st;
        std::memset(&st, 0, sizeof(st));
        const bool sok = mz_zip_reader_file_stat(&z, i, &st);
        p.boolean(sok).i32(int32_t(z.m_last_error));
        if (sok)
            put_stat(p, st);
        std::string   extra(c.extra_buf_size, '\0');
        const mz_uint ret = mz_zip_reader_get_extra(&z, i, extra.data(), c.extra_buf_size);
        p.u32(ret).str(extra.substr(0, std::min<size_t>(ret, c.extra_buf_size))).i32(int32_t(z.m_last_error));
        const bool sized = sok && st.m_uncomp_size <= MAX_EXTRACT && st.m_comp_size <= MAX_EXTRACT;
        p.boolean(sized);
        if (!sized)
            continue;
        {
            std::string buf(size_t(st.m_uncomp_size), '\0');
            const bool  r = mz_zip_reader_extract_to_mem(&z, i, buf.data(), buf.size(), 0);
            p.boolean(r).i32(int32_t(z.m_last_error)).u64(fnv1a(buf));
            p.boolean(buf.size() <= 300);
            if (buf.size() <= 300)
                p.str(buf);
        }
        if (st.m_uncomp_size > 0) {
            std::string buf(size_t(st.m_uncomp_size - 1), '\0');
            const bool  r = mz_zip_reader_extract_to_mem(&z, i, buf.data(), buf.size(), 0);
            p.boolean(r).i32(int32_t(z.m_last_error));
        }
        {
            std::string buf(size_t(st.m_comp_size), '\0');
            const bool  r = mz_zip_reader_extract_to_mem(&z, i, buf.data(), buf.size(), MZ_ZIP_FLAG_COMPRESSED_DATA);
            p.boolean(r).i32(int32_t(z.m_last_error)).u64(fnv1a(buf));
        }
        {
            CallbackLog log;
            log.fail_at  = c.fail_at;
            const bool r = mz_zip_reader_extract_to_callback(&z, i, callback, &log, 0);
            p.boolean(r).i32(int32_t(z.m_last_error)).u64(log.calls);
            p.bytes += log.first.bytes;
            p.u64(fnv1a(log.log.bytes)).u64(log.data.size()).u64(fnv1a(log.data));
        }
        {
            std::filesystem::remove(out_path, ec);
            const bool r = mz_zip_reader_extract_to_file(&z, i, out_path.string().c_str(), 0);
            p.boolean(r).i32(int32_t(z.m_last_error));
            std::string contents;
            bool        exists = false;
            {
                std::ifstream f(out_path, std::ios::binary);
                exists = bool(f);
                if (exists)
                    contents.assign(std::istreambuf_iterator<char>(f), std::istreambuf_iterator<char>());
            }
            p.boolean(exists).u64(contents.size()).u64(fnv1a(contents));
            std::filesystem::remove(out_path, ec);
        }
    }
    for (const Query &q : c.queries) {
        const int idx = mz_zip_reader_locate_file(&z, q.name.c_str(), q.has_comment ? q.comment.c_str() : nullptr, q.flags);
        p.i32(idx).i32(int32_t(z.m_last_error));
        uint64_t size = 16;
        if (idx >= 0) {
            mz_zip_archive_file_stat st;
            if (mz_zip_reader_file_stat(&z, mz_uint(idx), &st) && st.m_uncomp_size <= MAX_EXTRACT)
                size = st.m_uncomp_size;
        }
        std::string buf(size_t(size), '\0');
        const bool  r = mz_zip_reader_extract_file_to_mem(&z, q.name.c_str(), buf.data(), buf.size(), q.flags);
        p.u64(size).boolean(r).i32(int32_t(z.m_last_error)).u64(fnv1a(buf));
    }
    for (int k = 0; k < 2; ++k) {
        const bool r = how == 1 || how == 2 ? mz_zip_reader_end(&z) : close_zip_reader(&z);
        p.boolean(r).i32(int32_t(z.m_last_error));
    }
    p.u32(mz_zip_reader_get_num_files(&z));
    std::filesystem::remove(archive_path, ec);
    add(sets, "ZARC", p, committed);
}

std::vector<Query> queries_for(const std::vector<std::string> &names, SplitMix &g)
{
    std::vector<Query> q;
    for (const std::string &n : names) {
        q.push_back({n});
        std::string up = n;
        for (char &ch : up)
            if (ch >= 'a' && ch <= 'z')
                ch = char(ch - 'a' + 'A');
        if (up != n)
            q.push_back({up});
        if (g.below(3) == 0)
            q.push_back({up, MZ_ZIP_FLAG_CASE_SENSITIVE});
        if (g.below(4) == 0) {
            const size_t slash = n.find_last_of('/');
            if (slash != std::string::npos && slash + 1 < n.size())
                q.push_back({n.substr(slash + 1), MZ_ZIP_FLAG_IGNORE_PATH});
        }
    }
    q.push_back({"missing.txt"});
    q.push_back({""});
    q.push_back({"missing.txt", MZ_ZIP_FLAG_CASE_SENSITIVE});
    return q;
}

std::vector<ArchiveCase> archives(SplitMix &g)
{
    std::vector<ArchiveCase> out;
    auto make = [&](const std::string &label, const std::string &bytes, const std::vector<std::string> &names, int commit) {
        ArchiveCase c;
        c.label   = label;
        c.bytes   = bytes;
        c.queries = queries_for(names, g);
        c.commit  = commit;
        return c;
    };
    auto push = [&](const std::string &label, const std::string &bytes, const std::vector<std::string> &names, int commit) {
        out.push_back(make(label, bytes, names, commit));
    };

    const std::string small_model = model_text(g, 8);
    const std::string model       = model_text(g, 900);
    const std::string content_types =
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\n <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\n <Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/>\n</Types>\n";
    const std::string rels =
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n <Relationship Target=\"/3D/3dmodel.model\" Id=\"rel-1\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/>\n</Relationships>\n";

    // miniz's writer.
    push("writer: empty", write_zip({}), {"a"}, 3);
    push("writer: one stored", write_zip({{"3D/3dmodel.model", small_model, "", "", 0}}), {"3D/3dmodel.model"}, 3);
    push("writer: one deflated", write_zip({{"3D/3dmodel.model", small_model, "", "", 6}}), {"3D/3dmodel.model", "3d/3DMODEL.MODEL"}, 3);
    {
        std::vector<WEntry> e = {{"[Content_Types].xml", content_types, "", "", 6},
                                 {"_rels/.rels", rels, "", "", 6},
                                 {"3D/3dmodel.model", model, "", "", 6},
                                 {"Metadata/", "", "", "", 0},
                                 {"Metadata/plate_1.png", random_bytes(g, 3000), "", "", 0},
                                 {"Metadata/model_settings.config", "<config>\n  <object id=\"1\"/>\n</config>\n", "", "", 6}};
        std::vector<std::string> names;
        for (const WEntry &w : e)
            names.push_back(w.name);
        push("writer: 3mf", write_zip(e), names, 0);
        std::vector<WEntry> small = e;
        small[2].data             = small_model;
        small[4].data             = random_bytes(g, 200);
        push("writer: small 3mf", write_zip(small), names, 2);
        push("writer: small 3mf, zip64", write_zip(small, MZ_ZIP_FLAG_WRITE_ZIP64), names, 2);
    }
    {
        std::vector<WEntry>      e     = {{"Readme.TXT", "1", "", "", 0}, {"readme.txt", "22", "", "", 0}, {"README.txt", "333", "", "", 0},
                                          {"b", "b", "", "", 0},          {"B", "BB", "", "", 0},          {"a/b", "ab", "", "", 0},
                                          {"A/B/", "", "", "", 0},        {"z", "z", "c1", "", 0},         {"y", "y", "c1", "", 0}};
        std::vector<std::string> names = {"readme.txt", "README.TXT", "Readme.TXT", "b", "B", "a/B", "A/B/", "z", "y"};
        ArchiveCase              c     = make("writer: names alike but for case", write_zip(e), names, 2);
        c.queries.push_back({"y", 0, true, "c1"});
        c.queries.push_back({"z", 0, true, "c2"});
        c.queries.push_back({"y", 0, true, ""});
        c.queries.push_back({"b", MZ_ZIP_FLAG_IGNORE_PATH});
        ArchiveCase unsorted = c;
        unsorted.label       = "writer: names alike, not sorted";
        unsorted.init_flags  = MZ_ZIP_FLAG_DO_NOT_SORT_CENTRAL_DIRECTORY;
        unsorted.commit      = 1;
        out.push_back(c);
        out.push_back(unsorted);
    }
    {
        std::vector<WEntry>      e;
        std::vector<std::string> names;
        const char               alphabet[] = "abcXYZ019/_.";
        for (int i = 0; i < 150; ++i) {
            std::string  name;
            const size_t len = 1 + size_t(g.below(12));
            for (size_t k = 0; k < len; ++k)
                name.push_back(alphabet[g.below(sizeof(alphabet) - 1)]);
            if (name[0] == '/')
                name[0] = 'q';
            name += std::to_string(i);
            e.push_back({name, random_bytes(g, size_t(g.below(40))), "", "", int(g.below(2)) * 6});
            if (i % 9 == 0)
                names.push_back(name);
        }
        ArchiveCase c    = make("writer: 150 entries", write_zip(e), names, 0);
        c.extra_buf_size = 5;
        c.fail_at        = 0;
        ArchiveCase unsorted = c;
        unsorted.label       = "writer: 150 entries, not sorted";
        unsorted.init_flags  = MZ_ZIP_FLAG_DO_NOT_SORT_CENTRAL_DIRECTORY;
        out.push_back(c);
        out.push_back(unsorted);
    }
    {
        const std::string longname(700, 'n');
        std::string       field;
        put16(field, 0x7075);
        const std::string uname = "3D/3dmodel.model";
        put16(field, uint32_t(5 + uname.size()));
        field.push_back('\x01');
        put32(field, uint32_t(mz_crc32(0, reinterpret_cast<const mz_uint8 *>(uname.data()), uname.size())));
        field += uname;
        std::vector<WEntry> e = {{"c0", "x", "", "", 0},
                                 {"c5", "xx", "hello", "", 0},
                                 {"c600", "xxx", std::string(600, 'c'), "", 0},
                                 {longname, "long", "", "", 0},
                                 {"x1", "x", "", field, 0},
                                 {"x2", "y", "", std::string(1100, 'e'), 6}};
        ArchiveCase c     = make("writer: comments, a long name, extra fields", write_zip(e), {"c0", "c5", "c600", longname, "x1", "x2"}, 2);
        ArchiveCase none  = c;
        none.label        = "writer: extra fields into no buffer";
        none.extra_buf_size = 0;
        none.commit       = 1;
        ArchiveCase one   = c;
        one.label         = "writer: extra fields into one byte";
        one.extra_buf_size = 1;
        one.fail_at       = 1;
        one.commit        = 1;
        out.push_back(c);
        out.push_back(none);
        out.push_back(one);
    }
    {
        // Big entries: more than one read buffer and more than one ring.
        std::vector<WEntry> e = {{"3D/3dmodel.model", model_text(g, 5000), "", "", 6},
                                 {"random.bin", random_bytes(g, 150000), "", "", 6},
                                 {"stored.bin", random_bytes(g, 140000), "", "", 0},
                                 {"run.bin", std::string(300000, 'r'), "", "", 1}};
        ArchiveCase c = make("writer: big entries", write_zip(e), {"3D/3dmodel.model", "random.bin", "stored.bin", "run.bin"}, 0);
        c.fail_at     = 3;
        out.push_back(c);
    }

    // The builder.
    {
        RawZip z;
        z.entries = {deflated("3D/3dmodel.model", small_model), stored("Metadata/a.txt", "hello"), deflated("b.bin", random_bytes(g, 100))};
        const Built                    base  = build(z);
        const std::vector<std::string> names = {"3D/3dmodel.model", "Metadata/a.txt", "b.bin"};
        push("built: three entries", base.bytes, names, 3);
        for (size_t cut : {base.bytes.size() - 1, base.bytes.size() - 22, base.bytes.size() / 2, size_t(21), size_t(0)})
            push("built: cut to " + std::to_string(cut), base.bytes.substr(0, cut), names, 1);
        for (int i = 0; i < 30; ++i) {
            std::string s = base.bytes;
            for (int k = 0, nk = 1 + int(g.below(4)); k < nk; ++k)
                s[size_t(g.below(s.size()))] = char(g.next());
            push("built: bytes changed #" + std::to_string(i), s, names, i < 8 ? 1 : 0);
        }
        auto patched = [&](const std::string &label, size_t at, uint32_t v, bool wide) {
            std::string s = base.bytes;
            if (wide)
                patch32(s, at, v);
            else
                patch16(s, at, v);
            push("built: " + label, s, names, 1);
        };
        const size_t e = base.eocd, r0 = base.records[0], r1 = base.records[1], r2 = base.records[2];
        patched("this disk 1", e + 4, 1, false);
        {
            std::string s = base.bytes;
            patch16(s, e + 4, 1);
            patch16(s, e + 6, 1);
            push("built: both disks 1", s, names, 1);
        }
        patched("this disk 2", e + 4, 2, false);
        patched("entries on the disk differ", e + 8, 2, false);
        patched("directory too small", e + 12, 40, true);
        patched("directory past the end", e + 16, uint32_t(base.cdir_ofs + 10), true);
        patched("second record's signature", r1, 0x02014b51, true);
        patched("an entry on disk 0xFFFF", r1 + 34, 0xFFFF, false);
        patched("an entry on disk 1", r1 + 34, 1, false);
        patched("an entry on disk 3", r1 + 34, 3, false);
        patched("method 12", r0 + 10, 12, false);
        patched("encrypted", r0 + 8, 1, false);
        patched("strong encryption", r2 + 8, 64, false);
        patched("a patch", r0 + 8, 32, false);
        patched("the local directory masked", r1 + 8, 8192, false);
        patched("no compressed bytes", r2 + 20, 0, true);
        patched("stored sizes differ", r1 + 24, 7, true);
        {
            std::string s = base.bytes;
            patch32(s, r1 + 24, 3);
            patch32(s, r1 + 20, 3);
            push("built: stored, both sizes 3", s, names, 1);
            patch16(s, r1 + 12, 0);
            patch16(s, r1 + 14, 0);
            patch32(s, r1 + 20, 5);
            push("built: stored sizes differ, method and time zero", s, names, 1);
        }
        patched("the CRC wrong", r0 + 16, 0x12345678, true);
        patched("the stored CRC wrong", r1 + 16, 0x12345678, true);
        patched("the size too small", r0 + 24, uint32_t(small_model.size() - 10), true);
        patched("the size too large", r0 + 24, uint32_t(small_model.size() + 10), true);
        patched("the size far too large", r0 + 24, 0x7FFFFFF0u, true);
        patched("a directory by its attribute", r1 + 38, 0x10, true);
        patched("the local signature wrong", base.locals[1], 0x04034b51, true);
        patched("the local name past the end", base.locals[2] + 26, 0xFFFF, false);
        patched("the local header past the end", r2 + 42, uint32_t(base.bytes.size()), true);
        patched("the comment past the directory", r2 + 32, 0x100, false);
        {
            RawZip pz = z;
            pz.prefix = random_bytes(g, 100);
            push("built: junk before", build(pz).bytes, names, 1);
        }
        {
            RawZip lz  = z;
            lz.comment = std::string(4500, 'k');
            push("built: a comment of 4500 bytes", build(lz).bytes, names, 0);
            lz.comment = std::string(60000, 'k');
            push("built: a comment of 60000 bytes", build(lz).bytes, names, 0);
            lz.comment = std::string(65535, 'k');
            push("built: a comment of 65535 bytes", build(lz).bytes, names, 0);
            lz.comment = std::string(3000, 'k');
            lz.comment.replace(1000, 4, "PK\x05\x06", 4);
            push("built: a signature in the comment", build(lz).bytes, names, 0);
        }
        {
            RawZip m              = z;
            m.entries[1].flags    = 1 << 11;
            m.entries[0].made_by  = 0x031E;
            m.entries[0].int_attr = 1;
            m.entries[0].ext_attr = 0x81A40000u;
            push("built: UTF-8 flag and Unix attributes", build(m).bytes, names, 1);
        }
    }
    {
        // ZIP64.
        RawZip z;
        z.zip64                  = true;
        z.entries                = {deflated("3D/3dmodel.model", small_model), stored("a.txt", "zip64")};
        z.entries[0].zip64_sizes = true;
        const std::vector<std::string> names = {"3D/3dmodel.model", "a.txt"};
        const Built                    base  = build(z);
        push("built: zip64", base.bytes, names, 3);
        RawZip d          = z;
        d.z64_total_disks = 2;
        push("built: zip64 on two disks", build(d).bytes, names, 1);
        RawZip rs          = z;
        rs.z64_record_size = 40;
        push("built: zip64 record too small", build(rs).bytes, names, 1);
        {
            std::string s = base.bytes;
            patch32(s, base.eocd - 12, uint32_t(s.size()));
            push("built: zip64 locator past the end", s, names, 1);
            s = base.bytes;
            patch32(s, base.eocd - 20, 0x07064b51);
            push("built: zip64 locator signature wrong", s, names, 1);
        }
        RawZip ex = z;
        ex.zip64  = false;
        push("built: zip64 extra field, no zip64 records", build(ex).bytes, names, 1);
        RawZip sh                 = ex;
        sh.entries[0].zip64_sizes = false;
        sh.entries[0].comp        = 0xFFFFFFFFu;
        {
            std::string f;
            put16(f, 1);
            put16(f, 4);
            put32(f, 7);
            sh.entries[0].extra_central = f;
        }
        push("built: zip64 extra field too short", build(sh).bytes, names, 1);
        RawZip bad = sh;
        {
            std::string f;
            put16(f, 9);
            put16(f, 40);
            put32(f, 7);
            bad.entries[0].extra_central = f;
        }
        push("built: an extra field longer than its room", build(bad).bytes, names, 1);
        RawZip other = ex;
        {
            std::string f;
            put16(f, 0x5455);
            put16(f, 5);
            f += std::string(5, 't');
            other.entries[0].extra_central = f;
        }
        push("built: zip64 extra field after another", build(other).bytes, names, 1);
    }
    return out;
}

void record_arc(Sets &sets, const std::filesystem::path &dir)
{
    SplitMix                       g(0x2C3C0003u);
    const std::vector<ArchiveCase> cases = archives(g);
    for (const ArchiveCase &c : cases)
        for (uint8_t how = 0; how < 3; ++how)
            record_arc_one(sets, dir, c, how, (how == 1 && c.commit >= 1) || (how == 0 && c.commit >= 2) || (how == 2 && c.commit >= 3));
    ArchiveCase none;
    none.label = "no file";
    record_arc_one(sets, dir, none, 3, true);
    none.label = "a folder";
    record_arc_one(sets, dir, none, 4, true);
}

// ---- ZERR ------------------------------------------------------------------------------------

void record_err(Sets &sets)
{
    Payload p;
    p.u64(35);
    for (int code = -1; code <= 33; ++code)
        p.i32(code).str(mz_zip_get_error_string(mz_zip_error(code))).str(MZ_Archive::get_errorstr(mz_zip_error(code)));
    add(sets, "ZERR", p, true);
}

bool run_zip(const Options &options, std::string &error)
{
    Sets sets;
    head(sets, "zip");
    std::error_code             ec;
    const std::filesystem::path dir = std::filesystem::temp_directory_path(ec) / ("sketch-orca-probe-zip-" + std::to_string(_getpid()));
    if (ec || !std::filesystem::create_directories(dir, ec)) {
        error = "cannot make a temporary folder";
        return false;
    }
    bool ok = true;
    try {
        record_crc(sets);
        record_inf(sets);
        record_arc(sets, dir);
        record_err(sets);
    } catch (const std::exception &e) {
        ok    = false;
        error = e.what();
    }
    std::filesystem::remove_all(dir, ec);
    if (!ok)
        return false;
    return save(sets, options, "zip.sop1", error);
}

} // namespace

const Target &target()
{
    static const Target t{"zip", false, run_zip};
    return t;
}

} // namespace probe
