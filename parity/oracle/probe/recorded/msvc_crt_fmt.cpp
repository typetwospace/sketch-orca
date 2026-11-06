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
// The recording program of crates/orca-crt/tests/crt.rs and crates/orca-fmt/tests/fmt.rs, kept as
// it was run on 2026-11-06: it prints what MSVC 14.42's STL and the static UCRT give for seeded
// cases (to_chars, from_chars, strtod and strtof, printf, ostream << double and to_string, the
// float-to-integer casts and rand). The tests hold the lines they need as tables: MSVC_DOUBLE_CASTS,
// MSVC_FLOAT_CASTS and MSVC_RAND in crt.rs, MSVC_OSTREAM in fmt.rs. Built with the engine's flags
// in the pinned MSVC 14.42.34433 and Windows SDK 10.0.22000.0 environment (README.md here):
//
//   cl -nologo -O2 -Ob2 -GR -EHsc -MT -std:c++17 msvc_crt_fmt.cpp
//   msvc_crt_fmt [count]          (count: random cases per kind, 100000 when not given)
//
// parity probe's program (../main.cpp and the files beside it) records the same behaviours, and
// more, into parity/probes/; this one stays as the record of where the tests' tables came from.
#include <charconv>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <iomanip>
#include <locale>
#include <sstream>
#include <stdexcept>
#include <string>
#include <system_error>
#include <thread>
#include <vector>

// Float-to-integer casts, kept out of line so that cl compiles each cast as it would in the engine.
#define CASTS(SUFFIX, F)                                                                                       \
    __declspec(noinline) signed char to_i8##SUFFIX(F x) { return static_cast<signed char>(x); }               \
    __declspec(noinline) unsigned char to_u8##SUFFIX(F x) { return static_cast<unsigned char>(x); }           \
    __declspec(noinline) short to_i16##SUFFIX(F x) { return static_cast<short>(x); }                          \
    __declspec(noinline) unsigned short to_u16##SUFFIX(F x) { return static_cast<unsigned short>(x); }        \
    __declspec(noinline) int to_i32##SUFFIX(F x) { return static_cast<int>(x); }                              \
    __declspec(noinline) unsigned to_u32##SUFFIX(F x) { return static_cast<unsigned>(x); }                    \
    __declspec(noinline) long long to_i64##SUFFIX(F x) { return static_cast<long long>(x); }                  \
    __declspec(noinline) unsigned long long to_u64##SUFFIX(F x) { return static_cast<unsigned long long>(x); }
CASTS(_d, double)
CASTS(_f, float)

static uint64_t g_state = 0x0DDBA11ULL;
static uint64_t next_u64() {
    g_state += 0x9E3779B97F4A7C15ULL;
    uint64_t z = g_state;
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
    z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
    return z ^ (z >> 31);
}
static uint64_t below(uint64_t n) { return next_u64() % n; }

static double from_bits(uint64_t b) { double d; std::memcpy(&d, &b, 8); return d; }
static uint64_t to_bits(double d) { uint64_t b; std::memcpy(&b, &d, 8); return b; }
static float from_bits32(uint32_t b) { float f; std::memcpy(&f, &b, 4); return f; }
static uint32_t to_bits32(float f) { uint32_t b; std::memcpy(&b, &f, 4); return b; }

static const char* ec_name(std::errc ec) {
    if (ec == std::errc{}) return "ok";
    if (ec == std::errc::invalid_argument) return "inv";
    if (ec == std::errc::result_out_of_range) return "range";
    if (ec == std::errc::value_too_large) return "large";
    return "?";
}

// Escapes a string so it fits one tab-separated field: \\, \t, \n and bytes outside 0x20-0x7E as \xHH.
static std::string esc(const std::string& s) {
    std::string out;
    char buf[8];
    for (unsigned char c : s) {
        if (c == '\\') out += "\\\\";
        else if (c >= 0x20 && c <= 0x7E) out += static_cast<char>(c);
        else { std::snprintf(buf, sizeof buf, "\\x%02X", c); out += buf; }
    }
    return out;
}

static double random_double() {
    switch (below(7)) {
    case 0: {
        static const uint64_t specials[] = {0, 0x8000000000000000ULL, 0x7FF0000000000000ULL, 0xFFF0000000000000ULL,
            0x7FF8000000000000ULL, 0xFFF8000000000000ULL, 0x7FF0000000000001ULL, 0x7FF8000000000001ULL, 1,
            0x000FFFFFFFFFFFFFULL, 0x0010000000000000ULL, 0x7FEFFFFFFFFFFFFFULL, 0x3FF0000000000000ULL,
            0x3FB999999999999AULL, 0x4340000000000000ULL, 0x4340000000000001ULL, 0x44B52D02C7E14AF6ULL};
        return from_bits(specials[below(sizeof specials / sizeof specials[0])]);
    }
    case 1: case 2: return from_bits(next_u64());
    case 3: {
        char buf[64];
        std::snprintf(buf, sizeof buf, "%llue%d", static_cast<unsigned long long>(next_u64() >> below(64)),
            static_cast<int>(below(640)) - 330);
        return std::strtod(buf, nullptr);
    }
    case 4: return static_cast<double>(next_u64() >> below(64)) / static_cast<double>(1ULL << below(63));
    case 5: return from_bits(below(1ULL << 54));
    default: {
        char buf[64];
        std::snprintf(buf, sizeof buf, "%llu5e%d", static_cast<unsigned long long>(next_u64() >> (11 + below(53))),
            static_cast<int>(below(60)) - 30);
        return std::strtod(buf, nullptr);
    }
    }
}

static std::string random_number_text() {
    static const char* const pieces[] = {"0", "1", "9", "5", "00", ".", "e", "E", "+", "-", "p", "P", "x", "X",
        "inf", "INFINITY", "nan", "nan(", ")", "ind", "snan", "a", "f", "F", " ", "\t", "z", "_", "12345678901234567890",
        "4", "7", "2", "3", "e-", "e+", "1e308", "1e-324", "0x", "0X1.", "ffff", "c", "d"};
    std::string s;
    const int n = 1 + static_cast<int>(below(10));
    for (int i = 0; i < n; ++i) s += pieces[below(sizeof pieces / sizeof pieces[0])];
    return s;
}

static std::string random_decimal_text() {
    std::string s;
    if (below(4) == 0) s += '-';
    const int digits = 1 + static_cast<int>(below(below(10) == 0 ? 900 : 25));
    for (int i = 0; i < digits; ++i) s += static_cast<char>('0' + below(10));
    if (below(2)) s.insert(below(s.size() + 1), ".");
    if (below(2)) {
        s += below(2) ? 'e' : 'E';
        if (below(2)) s += below(2) ? '-' : '+';
        char buf[16];
        std::snprintf(buf, sizeof buf, "%d", static_cast<int>(below(below(10) == 0 ? 6000 : 400)));
        s += buf;
    }
    return s;
}

static std::string random_hex_text() {
    std::string s;
    if (below(4) == 0) s += '-';
    const int digits = 1 + static_cast<int>(below(below(10) == 0 ? 300 : 20));
    static const char hexits[] = "0123456789abcdefABCDEF";
    for (int i = 0; i < digits; ++i) s += hexits[below(22)];
    if (below(2)) s.insert(below(s.size() + 1), ".");
    if (below(2)) {
        s += below(2) ? 'p' : 'P';
        if (below(2)) s += below(2) ? '-' : '+';
        char buf[16];
        std::snprintf(buf, sizeof buf, "%d", static_cast<int>(below(2200)));
        s += buf;
    }
    return s;
}

struct comma_punct : std::numpunct<char> {
    char do_decimal_point() const override { return ','; }
    char do_thousands_sep() const override { return '.'; }
    std::string do_grouping() const override { return m_grouping; }
    std::string m_grouping;
    explicit comma_punct(std::string g) : std::numpunct<char>(1), m_grouping(std::move(g)) {}
};

int main(int argc, char** argv) {
    const int count = argc > 1 ? std::atoi(argv[1]) : 100000;
    FILE* out = stdout;
    char buf[2000];

    // to_chars: shortest, shortest per format, precision per format.
    for (int i = 0; i < count; ++i) {
        const double v = random_double();
        auto r = std::to_chars(buf, buf + sizeof buf, v);
        std::fprintf(out, "tc_d\t%016llX\t%s\t%.*s\n", static_cast<unsigned long long>(to_bits(v)), ec_name(r.ec),
            static_cast<int>(r.ptr - buf), buf);
        const float f = from_bits32(static_cast<uint32_t>(next_u64()));
        r = std::to_chars(buf, buf + sizeof buf, f);
        std::fprintf(out, "tc_f\t%08X\t%s\t%.*s\n", to_bits32(f), ec_name(r.ec), static_cast<int>(r.ptr - buf), buf);
        static const std::chars_format formats[] = {std::chars_format::scientific, std::chars_format::fixed,
            std::chars_format::hex, std::chars_format::general};
        const int fi = static_cast<int>(below(4));
        r = std::to_chars(buf, buf + sizeof buf, v, formats[fi]);
        std::fprintf(out, "tcf_d\t%016llX\t%d\t%s\t%.*s\n", static_cast<unsigned long long>(to_bits(v)), fi,
            ec_name(r.ec), static_cast<int>(r.ptr - buf), buf);
        r = std::to_chars(buf, buf + sizeof buf, f, formats[fi]);
        std::fprintf(out, "tcf_f\t%08X\t%d\t%s\t%.*s\n", to_bits32(f), fi, ec_name(r.ec),
            static_cast<int>(r.ptr - buf), buf);
        int p = static_cast<int>(below(40)) - 3;
        if (below(20) == 0) p = static_cast<int>(below(1200));
        r = std::to_chars(buf, buf + sizeof buf, v, formats[fi], p);
        std::fprintf(out, "tcp_d\t%016llX\t%d\t%d\t%s\t%.*s\n", static_cast<unsigned long long>(to_bits(v)), fi, p,
            ec_name(r.ec), r.ec == std::errc{} ? static_cast<int>(r.ptr - buf) : 0, buf);
        r = std::to_chars(buf, buf + sizeof buf, f, formats[fi], p);
        std::fprintf(out, "tcp_f\t%08X\t%d\t%d\t%s\t%.*s\n", to_bits32(f), fi, p, ec_name(r.ec),
            r.ec == std::errc{} ? static_cast<int>(r.ptr - buf) : 0, buf);
        // A short buffer.
        const int small = static_cast<int>(below(12));
        r = std::to_chars(buf, buf + small, v);
        std::fprintf(out, "tcs_d\t%016llX\t%d\t%s\t%.*s\n", static_cast<unsigned long long>(to_bits(v)), small,
            ec_name(r.ec), r.ec == std::errc{} ? static_cast<int>(r.ptr - buf) : 0, buf);
        // Integers.
        const long long iv = static_cast<long long>(next_u64() >> below(64)) * (below(2) ? -1 : 1);
        const int base = 2 + static_cast<int>(below(35));
        r = std::to_chars(buf, buf + sizeof buf, iv, base);
        std::fprintf(out, "tci\t%lld\t%d\t%s\t%.*s\n", iv, base, ec_name(r.ec), static_cast<int>(r.ptr - buf), buf);
    }

    // from_chars and strtod-family: text inputs.
    for (int i = 0; i < count; ++i) {
        std::string text;
        switch (below(5)) {
        case 0: text = random_number_text(); break;
        case 1: case 2: text = random_decimal_text(); break;
        case 3: text = random_hex_text(); break;
        default: {
            const double v = random_double();
            auto r = std::to_chars(buf, buf + sizeof buf, v, below(2) ? std::chars_format::general : std::chars_format::hex);
            text.assign(buf, r.ptr);
        }
        }
        static const std::chars_format formats[] = {std::chars_format::scientific, std::chars_format::fixed,
            std::chars_format::hex, std::chars_format::general};
        for (int fi = 0; fi < 4; ++fi) {
            double d = from_bits(0xDEADBEEFULL);
            auto r = std::from_chars(text.data(), text.data() + text.size(), d, formats[fi]);
            float f = from_bits32(0xDEADBEEF);
            auto rf = std::from_chars(text.data(), text.data() + text.size(), f, formats[fi]);
            std::fprintf(out, "fc\t%s\t%d\t%016llX\t%d\t%s\t%08X\t%d\t%s\n", esc(text).c_str(), fi,
                static_cast<unsigned long long>(to_bits(d)), static_cast<int>(r.ptr - text.data()), ec_name(r.ec),
                to_bits32(f), static_cast<int>(rf.ptr - text.data()), ec_name(rf.ec));
        }
        // stod, stof.
        std::string kind;
        double dv = 0;
        size_t idx = 0;
        try { dv = std::stod(text, &idx); kind = "ok"; } catch (const std::invalid_argument&) { kind = "inv"; } catch (const std::out_of_range&) { kind = "range"; }
        std::fprintf(out, "stod\t%s\t%s\t%016llX\t%zu\n", esc(text).c_str(), kind.c_str(),
            static_cast<unsigned long long>(to_bits(dv)), idx);
        float fv = 0;
        idx = 0;
        try { fv = std::stof(text, &idx); kind = "ok"; } catch (const std::invalid_argument&) { kind = "inv"; } catch (const std::out_of_range&) { kind = "range"; }
        std::fprintf(out, "stof\t%s\t%s\t%08X\t%zu\n", esc(text).c_str(), kind.c_str(), to_bits32(fv), idx);
    }

    // Integer from_chars and sto*.
    for (int i = 0; i < count; ++i) {
        std::string text;
        static const char* const pieces[] = {"-", "+", " ", "0", "0x", "0X", "1", "7", "8", "9", "f", "F", "z", "Z",
            "18446744073709551615", "18446744073709551616", "9223372036854775807", "9223372036854775808",
            "2147483647", "2147483648", "4294967295", "4294967296", "255", "256", "128", "32767", "65535"};
        const int n = 1 + static_cast<int>(below(4));
        for (int k = 0; k < n; ++k) text += pieces[below(sizeof pieces / sizeof pieces[0])];
        const int base = below(3) == 0 ? 10 : 2 + static_cast<int>(below(35));
        const char* first = text.data();
        const char* last = first + text.size();
#define FC_INT(T, NAME, FMT)                                                                                   \
    {                                                                                                          \
        T v = static_cast<T>(77);                                                                              \
        auto r = std::from_chars(first, last, v, base);                                                        \
        std::fprintf(out, "fci\t%s\t%s\t%d\t" FMT "\t%d\t%s\n", NAME, esc(text).c_str(), base, v,              \
            static_cast<int>(r.ptr - first), ec_name(r.ec));                                                   \
    }
        FC_INT(signed char, "i8", "%d")
        FC_INT(unsigned char, "u8", "%u")
        FC_INT(short, "i16", "%d")
        FC_INT(unsigned short, "u16", "%u")
        FC_INT(int, "i32", "%d")
        FC_INT(unsigned int, "u32", "%u")
        FC_INT(long long, "i64", "%lld")
        FC_INT(unsigned long long, "u64", "%llu")
        const int sbase = below(4) == 0 ? 0 : base;
#define STO(F, NAME, FMT, T)                                                                                   \
    {                                                                                                          \
        T v = 0;                                                                                               \
        size_t idx = 0;                                                                                        \
        std::string kind;                                                                                      \
        try { v = F(text, &idx, sbase); kind = "ok"; } catch (const std::invalid_argument&) { kind = "inv"; }  \
        catch (const std::out_of_range&) { kind = "range"; }                                                   \
        std::fprintf(out, "sto\t%s\t%s\t%d\t%s\t" FMT "\t%zu\n", NAME, esc(text).c_str(), sbase, kind.c_str(), \
            v, idx);                                                                                           \
    }
        STO(std::stoi, "stoi", "%d", int)
        STO(std::stol, "stol", "%ld", long)
        STO(std::stoul, "stoul", "%lu", unsigned long)
        STO(std::stoll, "stoll", "%lld", long long)
        STO(std::stoull, "stoull", "%llu", unsigned long long)
    }

    // ostream << double, with every flag, precision, width, fill and adjustment, and to_string.
    const std::locale comma3(std::locale::classic(), new comma_punct("\3"));
    const std::locale comma12(std::locale::classic(), new comma_punct("\1\2"));
    for (int i = 0; i < count; ++i) {
        const double v = below(8) == 0 ? static_cast<double>(from_bits32(static_cast<uint32_t>(next_u64())))
                                       : random_double();
        std::ostringstream os;
        const int field = static_cast<int>(below(4));
        const bool showpos = below(4) == 0, showpoint = below(4) == 0, upper = below(4) == 0;
        const int adjust = static_cast<int>(below(4));
        long long precision = static_cast<long long>(below(25)) - 2;
        if (below(30) == 0) precision = static_cast<long long>(below(400));
        const long long width = below(3) == 0 ? static_cast<long long>(below(40)) : 0;
        const char fill = below(3) == 0 ? '*' : ' ';
        const int loc = below(10) == 0 ? 1 + static_cast<int>(below(2)) : 0;
        if (loc == 1) os.imbue(comma3);
        if (loc == 2) os.imbue(comma12);
        std::ios_base::fmtflags flags{};
        if (field == 1) flags |= std::ios_base::fixed;
        if (field == 2) flags |= std::ios_base::scientific;
        if (field == 3) flags |= std::ios_base::fixed | std::ios_base::scientific;
        if (showpos) flags |= std::ios_base::showpos;
        if (showpoint) flags |= std::ios_base::showpoint;
        if (upper) flags |= std::ios_base::uppercase;
        if (adjust == 1) flags |= std::ios_base::left;
        if (adjust == 2) flags |= std::ios_base::internal;
        if (adjust == 3) flags |= std::ios_base::right;
        os.flags(flags);
        os.precision(precision);
        os.width(width);
        os.fill(fill);
        os << v << '|';
        std::fprintf(out, "os\t%016llX\t%d\t%d\t%d\t%d\t%d\t%lld\t%lld\t%c\t%d\t%s\n",
            static_cast<unsigned long long>(to_bits(v)), field, showpos, showpoint, upper, adjust, precision, width,
            fill, loc, esc(os.str()).c_str());
        std::fprintf(out, "ts\t%016llX\t%s\n", static_cast<unsigned long long>(to_bits(v)), std::to_string(v).c_str());
    }

    // Float-to-integer casts: edges and random values, through out-of-line functions called by pointer.
    {
        std::vector<double> ds = {0.0, -0.0, 0.5, -0.5, 0.999, -0.999, 1.0, -1.0, 127.0, 127.9, 128.0, -128.0, -128.9,
            -129.0, 255.0, 255.9, 256.0, 32767.0, 32768.0, -32768.0, -32769.0, 65535.0, 65536.0, 70000.0, -70000.0,
            2147483647.0, 2147483647.5, 2147483648.0, -2147483648.0, -2147483648.5, -2147483649.0, 4294967295.0,
            4294967295.5, 4294967296.0, 1e10, -1e10, 9223372036854774784.0, 9223372036854775808.0,
            -9223372036854775808.0, -9223372036854777856.0, 18446744073709549568.0, 18446744073709551616.0,
            36893488147419103232.0, 1e19, 1e20, 1e30, -1e30, 1e308, -1e308, 5e-324, -5e-324};
        ds.push_back(from_bits(0x7FF0000000000000ULL));
        ds.push_back(from_bits(0xFFF0000000000000ULL));
        ds.push_back(from_bits(0x7FF8000000000000ULL));
        ds.push_back(from_bits(0xFFF8000000000000ULL));
        ds.push_back(from_bits(0x7FF0000000000001ULL));
        for (int i = 0; i < count; ++i) {
            double v = random_double();
            if (below(2)) v = std::ldexp(static_cast<double>(static_cast<int64_t>(next_u64())), -static_cast<int>(below(70)));
            ds.push_back(v);
        }
        signed char (*volatile p_i8)(double) = to_i8_d;
        unsigned char (*volatile p_u8)(double) = to_u8_d;
        short (*volatile p_i16)(double) = to_i16_d;
        unsigned short (*volatile p_u16)(double) = to_u16_d;
        int (*volatile p_i32)(double) = to_i32_d;
        unsigned (*volatile p_u32)(double) = to_u32_d;
        long long (*volatile p_i64)(double) = to_i64_d;
        unsigned long long (*volatile p_u64)(double) = to_u64_d;
        for (double v : ds) {
            std::fprintf(out, "cvd\t%016llX\t%d\t%u\t%d\t%u\t%d\t%u\t%lld\t%llu\n", static_cast<unsigned long long>(to_bits(v)),
                p_i8(v), p_u8(v), p_i16(v), p_u16(v), p_i32(v), p_u32(v), p_i64(v), p_u64(v));
        }
        signed char (*volatile q_i8)(float) = to_i8_f;
        unsigned char (*volatile q_u8)(float) = to_u8_f;
        short (*volatile q_i16)(float) = to_i16_f;
        unsigned short (*volatile q_u16)(float) = to_u16_f;
        int (*volatile q_i32)(float) = to_i32_f;
        unsigned (*volatile q_u32)(float) = to_u32_f;
        long long (*volatile q_i64)(float) = to_i64_f;
        unsigned long long (*volatile q_u64)(float) = to_u64_f;
        for (double d : ds) {
            const float v = below(3) == 0 ? from_bits32(static_cast<uint32_t>(next_u64())) : static_cast<float>(d);
            std::fprintf(out, "cvf\t%08X\t%d\t%u\t%d\t%u\t%d\t%u\t%lld\t%llu\n", to_bits32(v), q_i8(v), q_u8(v),
                q_i16(v), q_u16(v), q_i32(v), q_u32(v), q_i64(v), q_u64(v));
        }
    }

    // rand: a new thread's sequence, then after srand of several seeds.
    {
        std::thread t([out] {
            std::fprintf(out, "rand\tfresh");
            for (int i = 0; i < 40; ++i) std::fprintf(out, "\t%d", std::rand());
            std::fprintf(out, "\n");
        });
        t.join();
        static const unsigned seeds[] = {0, 1, 2, 42, 12345, 0x7FFFFFFF, 0x80000000u, 0xFFFFFFFFu};
        for (unsigned seed : seeds) {
            std::srand(seed);
            std::fprintf(out, "rand\t%u", seed);
            for (int i = 0; i < 40; ++i) std::fprintf(out, "\t%d", std::rand());
            std::fprintf(out, "\n");
        }
    }
    return 0;
}
