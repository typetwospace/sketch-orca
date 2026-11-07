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
// cstdio.sop1: the static UCRT's stdio as the engine's model readers use it (parity/SOP1.md, CSCN
// and CFIL), recorded by the o2-mesh task (2026-11-07); orca-fmt's tests/probes.rs replays it
// against its stdio and scanf modules. Two records:
//
//   CSCN  sscanf of one input with one of the formats admesh passes (the table FORMATS, which the
//         Rust test holds in the same order): the count it returns and what each argument holds
//         after the call (a float's or double's bits, a char buffer's every byte), every argument
//         set to a sentinel first so that a conversion that stores nothing shows.
//   CFIL  a file written to a temporary folder, opened with "rb", "r" or "rt", and a script of
//         calls on it (getc, fgets, fscanf, fread, ftell, fseek, rewind, feof), each with its
//         result (fread's buffer whole for a binary stream, its whole items read for a text
//         stream: see FREAD). Text streams take no ftell and no fseek but to the start, as
//         orca-fmt leaves those out (no reader calls them).
//
// The inputs: numbers in every spelling the UCRT's strtod reads or stops in, ASCII STL text with
// each line end (LF, CR LF, CR, CR CR LF, LF CR), a Ctrl-Z anywhere and no final newline, and
// seeded random bytes. Nothing recorded depends on the temporary folder's path.
#include "probe.hpp"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <initializer_list>
#include <string>
#include <vector>

#include <process.h>

namespace probe {

namespace {

// One format and its arguments: 'f' float, 'd' double, 's' char[32], 'S' char[256].
struct Format
{
    const char *text;
    const char *args;
};

// The formats, by index (orca-fmt's tests/probes.rs, FORMATS, holds the same list).
const Format FORMATS[] = {
    {"%f", "f"},
    {"%lf", "d"},
    {" vertex %f %f %f", "fff"},
    {" vertex %f %f %f ", "fff"},
    {" facet normal %31s %31s %31s", "sss"},
    {" outer loop", ""},
    {" endsolid%*[^\n]\n", ""},
    {" solid%*[^\n]\n", ""},
    {" solid %[^\n]", "S"},
    {" ", ""},
    {"%f %f %f", "fff"},
};
constexpr int NUM_FORMATS = int(sizeof(FORMATS) / sizeof(FORMATS[0]));

constexpr uint32_t FLOAT_SENTINEL  = 0x12345678u;
constexpr uint64_t DOUBLE_SENTINEL = 0x123456789ABCDEF0ull;
constexpr char     CHAR_SENTINEL   = 0x55;

// The arguments of one call, set to their sentinels.
struct Args
{
    float               f[3];
    double              d[3];
    char                s[3][32];
    char                big[256];
    std::vector<void *> ptrs;
    explicit Args(const Format &fmt)
    {
        for (int i = 0; i < 3; ++i) {
            f[i] = from_bits32(FLOAT_SENTINEL);
            d[i] = from_bits(DOUBLE_SENTINEL);
            std::memset(s[i], CHAR_SENTINEL, sizeof(s[i]));
        }
        std::memset(big, CHAR_SENTINEL, sizeof(big));
        int nf = 0, nd = 0, ns = 0;
        for (const char *a = fmt.args; *a; ++a) {
            switch (*a) {
            case 'f': ptrs.push_back(&f[nf++]); break;
            case 'd': ptrs.push_back(&d[nd++]); break;
            case 's': ptrs.push_back(s[ns++]); break;
            default: ptrs.push_back(big); break;
            }
        }
    }
    // What each argument holds, in order.
    void put(Payload &p, const Format &fmt) const
    {
        int nf = 0, nd = 0, ns = 0;
        for (const char *a = fmt.args; *a; ++a) {
            switch (*a) {
            case 'f': p.u32(bits(f[nf++])); break;
            case 'd': p.u64(bits(d[nd++])); break;
            case 's':
                p.str(std::string(s[ns], 32));
                ++ns;
                break;
            default: p.str(std::string(big, 256)); break;
            }
        }
    }
};

int call_sscanf(const char *input, const Format &fmt, Args &a)
{
    switch (a.ptrs.size()) {
    case 0: return std::sscanf(input, fmt.text);
    case 1: return std::sscanf(input, fmt.text, a.ptrs[0]);
    default: return std::sscanf(input, fmt.text, a.ptrs[0], a.ptrs[1], a.ptrs[2]);
    }
}

int call_fscanf(FILE *fp, const Format &fmt, Args &a)
{
    switch (a.ptrs.size()) {
    case 0: return std::fscanf(fp, fmt.text);
    case 1: return std::fscanf(fp, fmt.text, a.ptrs[0]);
    default: return std::fscanf(fp, fmt.text, a.ptrs[0], a.ptrs[1], a.ptrs[2]);
    }
}

// Numbers in every spelling: whole ones, ones strtod stops inside, the special values.
const char *const NUMBERS[] = {
    "0", "-0", "+0", "1", "-1", "+1", "1.", ".5", "-.5", "+.5", "1.5", "1e5", "1E5", "1e-5", "1E+5", "1e+05", "12.5e-3",
    "1e", "1e+", "1e-", "1E", "1ex", "1e+x", "-", "+", ".", "-.", "+.", "..", "e5", "x",
    "0x1p3", "0X1P3", "0x1.8p-1", "0x.8p1", "0x10", "0xA.Bp0", "0x", "0X", "0x.", "0xg", "0x1p", "0x1p+", "0x1px",
    "inf", "INF", "Inf", "-inf", "+inf", "infinity", "INFINITY", "-Infinity", "infinit", "infin", "in", "i", "infix", "infinityx",
    "nan", "NAN", "-nan", "+nan", "nan(123)", "nan(ab_1)", "-nan(ind)", "nan()", "nan(", "nan(12", "nan(1 2)", "nanx", "na",
    "1.#INF", "1.#QNAN", "3.4028235e38", "3.4028236e38", "3.40282357e38", "1e39", "-1e39", "1e400", "1e-400",
    "1.17549435e-38", "1.1754942e-38", "1e-45", "1.4e-45", "7e-46", "7.1e-46", "2.8e-45", "1e-46",
    "16777217", "16777216.5", "0.1", "0.2", "0.3", "1.00000005960464477539", "1.0000000596046447753906250001",
    "123456789012345678901234567890", "0.000000000000000000000000000001234567890123456789",
    "4.9406564584124654e-324", "2.2250738585072014e-308", "1.7976931348623157e308", "1.7976931348623159e308",
    "00000000000000000000001.5", "1_000", "1,5", "1 5", "\t1", "\n\n1", "\v\f\r 1",
};
constexpr int NUM_NUMBERS = int(sizeof(NUMBERS) / sizeof(NUMBERS[0]));

const char *const LINE_ENDS[] = {"\n", "\r\n", "\r", "\r\r\n", "\n\r"};

std::string number(SplitMix &g)
{
    switch (g.below(4)) {
    case 0: return NUMBERS[g.below(NUM_NUMBERS)];
    case 1: {
        // A decimal of up to 30 digits, with a point and an exponent sometimes.
        std::string s;
        if (g.below(3) == 0)
            s += g.below(2) ? '-' : '+';
        const int digits = 1 + int(g.below(30));
        const int point  = g.below(2) ? int(g.below(uint64_t(digits) + 1)) : -1;
        for (int i = 0; i < digits; ++i) {
            if (i == point)
                s += '.';
            s += char('0' + g.below(10));
        }
        if (g.below(3) == 0) {
            s += g.below(2) ? 'e' : 'E';
            if (g.below(2))
                s += g.below(2) ? '-' : '+';
            s += std::to_string(g.below(60));
        }
        return s;
    }
    case 2: {
        char buf[64];
        std::snprintf(buf, sizeof(buf), g.below(2) ? "%.9g" : "%.17g", from_bits(g.next()));
        return buf;
    }
    default: {
        char buf[64];
        std::snprintf(buf, sizeof(buf), g.below(2) ? "%e" : "%f", double(from_bits32(uint32_t(g.next()))));
        return buf;
    }
    }
}

std::string space(SplitMix &g)
{
    static const char *const s[] = {" ", " ", "  ", "\t", " \t ", "\f", "\v"};
    return s[g.below(7)];
}

// An ASCII STL's text, `facets` facets, with line end `le` (-1: a random one per line).
std::string stl_text(SplitMix &g, int facets, int le)
{
    auto        eol = [&]() { return std::string(LINE_ENDS[le >= 0 ? le : int(g.below(5))]); };
    std::string t;
    t += "solid";
    if (g.below(3))
        t += " name" + std::to_string(g.below(1000));
    t += eol();
    for (int f = 0; f < facets; ++f) {
        t += space(g) + "facet normal " + number(g) + " " + number(g) + " " + number(g) + eol();
        t += space(g) + space(g) + "outer loop" + eol();
        for (int v = 0; v < 3; ++v)
            t += space(g) + space(g) + space(g) + "vertex " + number(g) + space(g) + number(g) + space(g) + number(g) + eol();
        t += space(g) + space(g) + (g.below(8) ? "endloop" : "endloop trailing text") + eol();
        if (g.below(6) == 0)
            t += eol();
        t += space(g) + (g.below(8) ? "endfacet" : "endfacet\t") + eol();
    }
    t += "endsolid";
    if (g.below(2))
        t += eol();
    return t;
}

void insert_ctrl_z(SplitMix &g, std::string &t)
{
    if (t.empty())
        t += '\x1a';
    else
        t.insert(size_t(g.below(t.size() + 1)), 1, '\x1a');
}

// One sscanf input for format `fi`: text that may or may not match it.
std::string scan_input(SplitMix &g, int fi)
{
    std::string s;
    switch (fi) {
    case 0:
    case 1: s = (g.below(4) == 0 ? space(g) : std::string()) + number(g) + (g.below(4) == 0 ? number(g) : std::string()); break;
    case 2:
    case 3:
        s = space(g) + (g.below(10) ? "vertex " : "vertx ") + number(g) + space(g) + number(g) + space(g) + number(g);
        if (g.below(2))
            s += LINE_ENDS[g.below(5)];
        break;
    case 4: s = space(g) + "facet normal " + number(g) + " " + number(g) + (g.below(8) ? " " + number(g) : std::string()); break;
    case 5: s = (g.below(2) ? std::string("\n  ") : std::string()) + (g.below(6) ? "outer loop" : "outer lop"); break;
    case 6: s = std::string(g.below(3) ? "  endsolid" : "endsolid name") + (g.below(2) ? " x\n" : "\n") + (g.below(2) ? "solid" : ""); break;
    case 7: s = std::string(g.below(3) ? "solid" : " solid name") + (g.below(2) ? "\r\n" : "\n") + "facet"; break;
    case 8: {
        // The line after "solid" stays under 255 bytes: a longer one overruns the buffer.
        s                = std::string(g.below(2) ? "solid " : "  solid\t") + "name";
        const int extra = int(g.below(200));
        for (int i = 0; i < extra; ++i)
            s += char(' ' + g.below(95));
        if (g.below(2))
            s += LINE_ENDS[g.below(5)] + std::string("facet");
        break;
    }
    case 9: s = space(g) + LINE_ENDS[g.below(5)] + space(g) + (g.below(2) ? "x" : ""); break;
    default: s = number(g) + space(g) + number(g) + space(g) + number(g); break;
    }
    return s;
}

void cscn(Payload &p, int fi, const std::string &input)
{
    const Format &fmt = FORMATS[fi];
    Args          a(fmt);
    const int     r = call_sscanf(input.c_str(), fmt, a);
    p.u8(uint8_t(fi)).str(input).i32(r);
    a.put(p, fmt);
}

// The file scripts' operations. SOLID_OVERFLOW: fscanf(" solid %[^\n]") from the start where the
// line after "solid" holds 256 bytes or more, which would overrun the 256-byte buffer: not called
// (the C call writes past it), only noted, and the stream left as it was.
enum Op : int32_t { GETC = 0, FGETS = 1, FSCANF = 2, FREAD = 3, FTELL = 4, FSEEK = 5, REWIND = 6, FEOF = 7, SOLID_OVERFLOW = 8 };

const char *const MODES[] = {"rb", "r", "rt"};

struct Script
{
    std::string                       raw;
    int                               mode = 0;
    std::vector<std::vector<int32_t>> ops; // each: the operation, then its arguments
};

bool run_script(const std::string &tmp, const Script &s, Payload &p, std::string &error)
{
    {
        FILE *w = std::fopen(tmp.c_str(), "wb");
        if (!w) {
            error = "cannot write the temporary file";
            return false;
        }
        const bool written = s.raw.empty() || std::fwrite(s.raw.data(), 1, s.raw.size(), w) == s.raw.size();
        std::fclose(w);
        if (!written) {
            error = "cannot write the temporary file";
            return false;
        }
    }
    FILE *fp = std::fopen(tmp.c_str(), MODES[s.mode]);
    if (!fp) {
        error = "cannot open the temporary file";
        return false;
    }
    p.str(s.raw).u8(uint8_t(s.mode)).u32(uint32_t(s.ops.size()));
    std::vector<char> buf(4096);
    for (const std::vector<int32_t> &op : s.ops) {
        p.u8(uint8_t(op[0]));
        switch (op[0]) {
        case GETC: p.i32(std::getc(fp)); break;
        case FGETS: {
            const int n = op[1];
            std::memset(buf.data(), CHAR_SENTINEL, buf.size());
            const bool   nonnull = std::fgets(buf.data(), n, fp) != nullptr;
            const size_t k       = strnlen(buf.data(), size_t(n));
            p.i32(n).boolean(nonnull).str(std::string(buf.data(), k < size_t(n) ? k + 1 : size_t(n)));
            break;
        }
        case FSCANF: {
            const Format &fmt = FORMATS[op[1]];
            Args          a(fmt);
            const int     r = call_fscanf(fp, fmt, a);
            p.u8(uint8_t(op[1])).i32(r);
            a.put(p, fmt);
            break;
        }
        case FREAD: {
            const size_t size = size_t(op[1]), count = size_t(op[2]);
            std::memset(buf.data(), CHAR_SENTINEL, buf.size());
            const size_t r = std::fread(buf.data(), size, count, fp);
            // A binary stream: the whole buffer. A text stream: the whole items read only. Asked
            // for 512 bytes or more at its start, the UCRT reads the file straight into the
            // buffer and translates it there, leaving the file's bytes after what it returns:
            // bytes C leaves unspecified (C11 7.21.8.1) and no reader looks at (objparser.cpp
            // reads the count fread returns), which orca-fmt does not write.
            const size_t keep = s.mode == 0 ? size * count : r * size;
            p.u32(uint32_t(size)).u32(uint32_t(count)).u64(r).str(std::string(buf.data(), keep));
            break;
        }
        case FTELL: p.i32(int32_t(std::ftell(fp))); break;
        case FSEEK: p.i32(op[1]).i32(op[2]).i32(std::fseek(fp, long(op[1]), op[2])); break;
        case REWIND: std::rewind(fp); break;
        case FEOF: p.boolean(std::feof(fp) != 0); break;
        default: break;
        }
    }
    std::fclose(fp);
    return true;
}

void random_op(SplitMix &g, Script &s, uint64_t max_fgets, uint64_t max_size, uint64_t max_count)
{
    switch (g.below(8)) {
    case 0: s.ops.push_back({GETC}); break;
    // fgets with n of 2 to 2047 (the readers pass 100 and 2047).
    case 1: s.ops.push_back({FGETS, int32_t(2 + g.below(g.below(4) == 0 ? 2046 : max_fgets))}); break;
    case 2: {
        // Any format but " solid %[^\n]", whose line could overrun its buffer anywhere.
        const int32_t fi = int32_t(g.below(NUM_FORMATS));
        s.ops.push_back({FSCANF, fi == 8 ? 9 : fi});
        break;
    }
    case 3: s.ops.push_back({FREAD, int32_t(1 + g.below(max_size)), int32_t(g.below(max_count))}); break;
    case 4:
        if (s.mode == 0)
            s.ops.push_back({FTELL});
        else
            s.ops.push_back({FEOF});
        break;
    case 5:
        if (s.mode == 0) {
            // Never before the start: a failed fseek is left out (no reader's seek fails).
            const int whence = int(g.below(3));
            s.ops.push_back({FSEEK, int32_t(g.below(uint64_t(s.raw.size()) + 20)), whence});
        } else
            s.ops.push_back({FSEEK, 0, SEEK_SET});
        break;
    case 6: s.ops.push_back({REWIND}); break;
    default: s.ops.push_back({FEOF}); break;
    }
}

// The length of the line " solid %[^\n]" would store reading `raw` from its start in `mode`
// (text modes: CR LF read as LF, the first Ctrl-Z the end), or -1 when it stores nothing.
long solid_line_length(const std::string &raw, int mode)
{
    std::string data;
    if (mode == 0)
        data = raw;
    else
        for (size_t i = 0; i < raw.size() && raw[i] != '\x1a'; ++i) {
            if (raw[i] == '\r' && i + 1 < raw.size() && raw[i + 1] == '\n')
                continue;
            data += raw[i];
        }
    auto   space = [](char c) { return c == ' ' || (c >= '\t' && c <= '\r'); };
    size_t i     = 0;
    while (i < data.size() && space(data[i]))
        ++i;
    if (data.compare(i, 5, "solid") != 0)
        return -1;
    i += 5;
    while (i < data.size() && space(data[i]))
        ++i;
    size_t n = 0;
    while (i + n < data.size() && data[i + n] != '\n')
        ++n;
    return n == 0 ? -1 : long(n);
}

// The reads admesh makes of an ASCII STL, then some random ones.
Script reader_script(SplitMix &g, std::string raw, int mode, bool random_tail)
{
    Script s;
    s.raw  = std::move(raw);
    s.mode = mode;
    auto op = [&](std::initializer_list<int32_t> v) { s.ops.emplace_back(v); };
    if (mode == 0) {
        op({FSEEK, 0, SEEK_END});
        op({FTELL});
        op({FSEEK, 84, SEEK_SET});
        op({FTELL});
    }
    op({REWIND});
    // stl_open_count_facets: the lines counted with fgets(100), the header read by getc.
    for (int i = 0; i < 6; ++i)
        op({FGETS, 100});
    op({REWIND});
    for (int i = 0; i < 12; ++i)
        op({GETC});
    op({REWIND});
    op({solid_line_length(s.raw, mode) >= 256 ? SOLID_OVERFLOW : FSCANF, 8});
    op({REWIND});
    // stl_read: each facet's calls.
    for (int facet = 0; facet < 3; ++facet) {
        op({FSCANF, 6});
        op({FSCANF, 7});
        op({FSCANF, 4});
        op({FSCANF, 5});
        op({FSCANF, 2});
        op({FSCANF, 2});
        op({FSCANF, 3});
        op({FGETS, 2047});
        op({FSCANF, 9});
        op({FGETS, 2047});
        op({FEOF});
    }
    if (random_tail) {
        const int extra = 4 + int(g.below(20));
        for (int i = 0; i < extra; ++i)
            random_op(g, s, 40, 8, 64);
    }
    return s;
}

Script random_bytes_script(SplitMix &g)
{
    Script       s;
    const size_t n = size_t(g.below(g.below(4) == 0 ? 9000 : 300));
    for (size_t i = 0; i < n; ++i) {
        const uint64_t k = g.below(10);
        s.raw += k < 2 ? '\r' : k < 4 ? '\n' : k == 4 ? '\x1a' : char(g.below(256));
    }
    s.mode        = int(g.below(3));
    const int ops = 8 + int(g.below(40));
    for (int i = 0; i < ops; ++i)
        random_op(g, s, 64, 16, 256);
    return s;
}

bool run_cstdio(const Options &options, std::string &error)
{
    Sets sets;
    head(sets, "cstdio");

    // CSCN: every number with %f and %lf, then each format on seeded inputs.
    {
        SplitMix g(0xC5CA4F00u);
        for (int fi = 0; fi < 2; ++fi)
            for (int i = 0; i < NUM_NUMBERS; ++i) {
                Payload p;
                cscn(p, fi, NUMBERS[i]);
                sets.full.add("CSCN", p);
                sets.committed.add("CSCN", p);
            }
        for (int fi = 0; fi < NUM_FORMATS; ++fi)
            for (int i = 0; i < 600; ++i) {
                Payload p;
                cscn(p, fi, scan_input(g, fi));
                sets.full.add("CSCN", p);
                if (i < 40)
                    sets.committed.add("CSCN", p);
            }
    }

    // CFIL: the STL text with each line end, a Ctrl-Z, no final newline; then random bytes.
    std::error_code             ec;
    const std::filesystem::path dir = std::filesystem::temp_directory_path(ec) / ("sketch-orca-probe-cstdio-" + std::to_string(_getpid()));
    if (ec || !std::filesystem::create_directories(dir, ec)) {
        error = "cannot make a temporary folder";
        return false;
    }
    const std::string tmp = (dir / "file.bin").string();
    bool              ok  = true;
    {
        std::vector<Script> edges;
        for (int le = 0; le < 5; ++le)
            for (int mode = 0; mode < 3; ++mode) {
                SplitMix gg(0x1E00u + uint64_t(le) * 3 + uint64_t(mode));
                edges.push_back(reader_script(gg, stl_text(gg, 3, le), mode, false));
            }
        for (int mode = 0; mode < 3; ++mode) {
            SplitMix    gg(0x2A00u + uint64_t(mode));
            std::string t = stl_text(gg, 3, -1);
            insert_ctrl_z(gg, t);
            edges.push_back(reader_script(gg, t, mode, true));
            edges.push_back(reader_script(gg, std::string(), mode, true));
            edges.push_back(reader_script(gg, std::string("solid"), mode, true));
            edges.push_back(reader_script(gg, std::string("\x1a"), mode, true));
            edges.push_back(reader_script(gg, std::string("\r"), mode, true));
            edges.push_back(reader_script(gg, std::string("a\r\x1a\nb"), mode, true));
        }
        for (const Script &s : edges) {
            Payload p;
            if (!(ok = run_script(tmp, s, p, error)))
                break;
            sets.full.add("CFIL", p);
            sets.committed.add("CFIL", p);
        }
        SplitMix g(0xC5F11E00u);
        for (int i = 0; ok && i < 1500; ++i) {
            Script s;
            if (g.below(2)) {
                std::string t = stl_text(g, 1 + int(g.below(4)), g.below(3) ? int(g.below(5)) : -1);
                if (g.below(4) == 0)
                    insert_ctrl_z(g, t);
                if (g.below(4) == 0 && !t.empty())
                    t.resize(size_t(g.below(t.size())));
                const int mode = int(g.below(3));
                s              = reader_script(g, t, mode, true);
            } else
                s = random_bytes_script(g);
            Payload p;
            if (!(ok = run_script(tmp, s, p, error)))
                break;
            sets.full.add("CFIL", p);
            if (i < 30 && p.bytes.size() <= 8192)
                sets.committed.add("CFIL", p);
        }
    }
    std::filesystem::remove_all(dir, ec);
    if (!ok)
        return false;
    return save(sets, options, "cstdio.sop1", error);
}

} // namespace

const Target &target()
{
    static const Target t{"cstdio", false, run_cstdio};
    return t;
}

} // namespace probe
