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
// fmt.sop1: how the engine's runtime writes and reads numbers, in the "C" locale it runs in.
//
// - PRTF: snprintf with %d, %i, %u, %x, %X, %o, %e, %E, %f, %F, %g, %G, %a and %A, with flags,
//   widths and precisions (and ll for the integers), on seeded values; the text and the count.
// - TOCH: std::to_chars for double and float: shortest, shortest in each chars_format, with a
//   precision in each, and into short buffers.
// - OSTR and TOST: std::ostream << double under every floatfield, flag, precision, width, fill and
//   adjustment (and two locales with grouping), and std::to_string(double).
// - PARS: strtod and strtof (with errno), std::stod and std::stof (with what they throw), and
//   std::from_chars for double and float in each chars_format, on seeded text.
#include <cerrno>
#include <charconv>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <locale>
#include <sstream>
#include <stdexcept>
#include <string>
#include <system_error>
#include <vector>

#include "probe.hpp"

namespace probe {

namespace {

constexpr size_t PRTF_FORMATS_FULL      = 6000;
constexpr size_t PRTF_FORMATS_COMMITTED = 700;
constexpr size_t PRTF_VALUES_FULL       = 48;
constexpr size_t PRTF_VALUES_COMMITTED  = 6;
constexpr size_t CASES_FULL             = 40000;
constexpr size_t CASES_COMMITTED        = 600;

const uint64_t SPECIALS[] = {0, 0x8000000000000000ull, 0x7FF0000000000000ull, 0xFFF0000000000000ull, 0x7FF8000000000000ull, 0xFFF8000000000000ull,
                             0x7FF0000000000001ull, 0x7FF8000000000001ull, 1, 0x000FFFFFFFFFFFFFull, 0x0010000000000000ull, 0x7FEFFFFFFFFFFFFFull,
                             0x3FF0000000000000ull, 0x3FB999999999999Aull, 0x4340000000000000ull, 0x4340000000000001ull, 0x44B52D02C7E14AF6ull,
                             0x3FE0000000000000ull, 0x3FF8000000000000ull, 0x4004000000000000ull, 0x3F50624DD2F1A9FCull, 0x405EDD3C07EE0B0Bull};

double random_double(SplitMix &g)
{
    switch (g.below(8)) {
    case 0: return from_bits(SPECIALS[g.below(sizeof SPECIALS / sizeof SPECIALS[0])]);
    case 1:
    case 2: return from_bits(g.next());
    case 3: {
        char buf[64];
        std::snprintf(buf, sizeof buf, "%llue%d", (unsigned long long) (g.next() >> g.below(64)), int(g.below(640)) - 330);
        return std::strtod(buf, nullptr);
    }
    case 4: return double(g.next() >> g.below(64)) / double(1ull << g.below(63));
    case 5: return from_bits(g.below(1ull << 54));
    case 6: return double(int64_t(g.below(200001)) - 100000) / 1000.0;
    default: {
        char buf[64];
        std::snprintf(buf, sizeof buf, "%llu5e%d", (unsigned long long) (g.next() >> (11 + g.below(53))), int(g.below(60)) - 30);
        return std::strtod(buf, nullptr);
    }
    }
}

float random_float(SplitMix &g) { return g.below(2) ? from_bits32(uint32_t(g.next())) : float(random_double(g)); }

int errc_value(std::errc ec) { return int(ec); }

// ---------------------------------------------------------------- printf

// A conversion: its letter, whether it takes an integer, and whether that integer is unsigned.
struct Conversion
{
    char letter;
    bool integer;
    bool is_unsigned;
};

const Conversion CONVERSIONS[] = {{'d', true, false}, {'i', true, false}, {'u', true, true},   {'x', true, true},   {'X', true, true},
                                  {'o', true, true},  {'e', false, false}, {'E', false, false}, {'f', false, false}, {'F', false, false},
                                  {'g', false, false}, {'G', false, false}, {'a', false, false}, {'A', false, false}};

std::string random_format(SplitMix &g, const Conversion &c, bool ll)
{
    std::string f = "%";
    for (char flag : {'-', '+', ' ', '#', '0'})
        if (g.below(4) == 0)
            f += flag;
    const char *const widths[] = {"", "", "1", "5", "8", "12", "25", "40"};
    f += widths[g.below(sizeof widths / sizeof widths[0])];
    const char *const precisions[] = {"", "", ".", ".0", ".1", ".2", ".3", ".6", ".9", ".13", ".17", ".25", ".40", ".60"};
    f += precisions[g.below(sizeof precisions / sizeof precisions[0])];
    if (c.integer && ll)
        f += "ll";
    f += c.letter;
    // Text around the conversion, as the engine's formats have.
    if (g.below(3) == 0)
        f = "x=" + f + ";";
    return f;
}

uint64_t random_integer(SplitMix &g, bool wide)
{
    switch (g.below(5)) {
    case 0: return uint64_t(int64_t(g.below(21)) - 10);
    case 1: {
        const uint64_t edges[] = {0, 1, 0x7FFFFFFFull, 0x80000000ull, 0xFFFFFFFFull, 0x7FFFFFFFFFFFFFFFull, 0x8000000000000000ull, ~0ull};
        return edges[g.below(sizeof edges / sizeof edges[0])];
    }
    default: return wide ? g.next() >> g.below(64) : uint64_t(uint32_t(g.next() >> g.below(32)));
    }
}

void record_printf(Sets &sets)
{
    SplitMix g(0x5052544600000001ull);
    char     buf[4096];
    for (size_t i = 0; i < PRTF_FORMATS_FULL; ++i) {
        const Conversion &c   = CONVERSIONS[g.below(sizeof CONVERSIONS / sizeof CONVERSIONS[0])];
        const bool        ll  = c.integer && g.below(3) == 0;
        const std::string fmt = random_format(g, c, ll);
        // The argument as C++ passes it: int or unsigned (sign-extended or zero-extended to 64
        // bits here), long long or unsigned long long, or double.
        const char *type = !c.integer ? "f64" : ll ? (c.is_unsigned ? "u64" : "i64") : (c.is_unsigned ? "u32" : "i32");
        std::vector<uint64_t>    args;
        std::vector<std::string> outs;
        std::vector<int32_t>     counts;
        for (size_t k = 0; k < PRTF_VALUES_FULL; ++k) {
            uint64_t arg = 0;
            int      n   = 0;
            if (!c.integer) {
                const double v = random_double(g);
                arg            = bits(v);
                n              = std::snprintf(buf, sizeof buf, fmt.c_str(), v);
            } else if (ll) {
                arg = random_integer(g, true);
                n   = c.is_unsigned ? std::snprintf(buf, sizeof buf, fmt.c_str(), (unsigned long long) arg)
                                    : std::snprintf(buf, sizeof buf, fmt.c_str(), (long long) arg);
            } else if (c.is_unsigned) {
                const unsigned v = unsigned(random_integer(g, false));
                arg              = v;
                n                = std::snprintf(buf, sizeof buf, fmt.c_str(), v);
            } else {
                const int v = int(uint32_t(random_integer(g, false)));
                arg         = uint64_t(int64_t(v));
                n           = std::snprintf(buf, sizeof buf, fmt.c_str(), v);
            }
            args.push_back(arg);
            outs.push_back(n >= 0 ? std::string(buf, size_t(n) < sizeof buf ? size_t(n) : sizeof buf - 1) : std::string());
            counts.push_back(n);
        }
        for (int which = 0; which < 2; ++which) {
            if (which == 0 && i >= PRTF_FORMATS_COMMITTED)
                continue;
            const size_t n = which == 0 ? PRTF_VALUES_COMMITTED : args.size();
            Payload      p;
            p.str(fmt).str(type).u64(n);
            for (size_t k = 0; k < n; ++k)
                p.u64(args[k]).str(outs[k]).i32(counts[k]);
            (which == 0 ? sets.committed : sets.full).add("PRTF", p);
        }
    }
}

// ---------------------------------------------------------------- to_chars

const std::chars_format FORMATS[] = {std::chars_format::scientific, std::chars_format::fixed, std::chars_format::hex, std::chars_format::general};

struct CharsCase
{
    uint64_t    bits;
    int32_t     format; // -1: none (the shortest round trip); 0 scientific, 1 fixed, 2 hex, 3 general
    int32_t     precision;
    bool        has_precision;
    uint32_t    buffer;
    int32_t     errc;
    std::string text;
};

template<class F> CharsCase chars_case(F v, int fmt, bool has_precision, int precision, uint32_t buffer)
{
    static char buf[2400];
    std::to_chars_result r;
    if (fmt < 0)
        r = std::to_chars(buf, buf + buffer, v);
    else if (!has_precision)
        r = std::to_chars(buf, buf + buffer, v, FORMATS[fmt]);
    else
        r = std::to_chars(buf, buf + buffer, v, FORMATS[fmt], precision);
    CharsCase c;
    if constexpr (sizeof(F) == 8)
        c.bits = bits(double(v));
    else
        c.bits = bits(float(v));
    c.format        = fmt;
    c.precision     = precision;
    c.has_precision = has_precision;
    c.buffer        = buffer;
    c.errc          = errc_value(r.ec);
    c.text          = r.ec == std::errc{} ? std::string(buf, r.ptr) : std::string();
    return c;
}

void add_chars(Sets &sets, const char *kind, uint8_t width, const std::vector<CharsCase> &cases)
{
    for (int which = 0; which < 2; ++which) {
        const size_t n = which == 0 ? Sets::committed_count(cases.size(), CASES_COMMITTED) : cases.size();
        Payload      p;
        p.str(kind).u8(width).u64(n);
        for (size_t i = 0; i < n; ++i) {
            const CharsCase &c = cases[i];
            p.u64(c.bits).i32(c.format).boolean(c.has_precision).i32(c.precision).u32(c.buffer).i32(c.errc).str(c.text);
        }
        (which == 0 ? sets.committed : sets.full).add("TOCH", p);
    }
}

void record_to_chars(Sets &sets)
{
    SplitMix               g(0x544F434800000001ull);
    std::vector<CharsCase> sd, sf, fd, ff, pd, pf, small;
    for (size_t i = 0; i < CASES_FULL; ++i) {
        const double v = i < sizeof SPECIALS / sizeof SPECIALS[0] ? from_bits(SPECIALS[i]) : random_double(g);
        const float  f = random_float(g);
        sd.push_back(chars_case(v, -1, false, 0, 2400));
        sf.push_back(chars_case(f, -1, false, 0, 2400));
        const int fi = int(g.below(4));
        fd.push_back(chars_case(v, fi, false, 0, 2400));
        ff.push_back(chars_case(f, fi, false, 0, 2400));
        int p = int(g.below(40)) - 3;
        if (g.below(20) == 0)
            p = int(g.below(1100));
        pd.push_back(chars_case(v, fi, true, p, 2400));
        pf.push_back(chars_case(f, fi, true, p, 2400));
        small.push_back(chars_case(v, int(g.below(5)) - 1, false, 0, uint32_t(g.below(26))));
    }
    add_chars(sets, "shortest", 64, sd);
    add_chars(sets, "shortest", 32, sf);
    add_chars(sets, "format", 64, fd);
    add_chars(sets, "format", 32, ff);
    add_chars(sets, "precision", 64, pd);
    add_chars(sets, "precision", 32, pf);
    add_chars(sets, "short-buffer", 64, small);
}

// ---------------------------------------------------------------- ostream

struct comma_punct : std::numpunct<char>
{
    char        do_decimal_point() const override { return ','; }
    char        do_thousands_sep() const override { return '.'; }
    std::string do_grouping() const override { return m_grouping; }
    std::string m_grouping;
    explicit comma_punct(std::string g) : std::numpunct<char>(1), m_grouping(std::move(g)) {}
};

void record_ostream(Sets &sets)
{
    SplitMix          g(0x4F53545200000001ull);
    const std::locale comma3(std::locale::classic(), new comma_punct("\3"));
    const std::locale comma12(std::locale::classic(), new comma_punct("\1\2"));
    Payload           full, committed;
    std::vector<std::pair<uint64_t, std::string>> to_string_cases;
    const size_t      count = CASES_FULL;
    full.u64(count);
    committed.u64(CASES_COMMITTED);
    for (size_t i = 0; i < count; ++i) {
        const double v = g.below(8) == 0 ? double(from_bits32(uint32_t(g.next()))) : random_double(g);
        std::ostringstream os;
        const int  field    = int(g.below(4));
        const bool showpos  = g.below(4) == 0;
        const bool showpt   = g.below(4) == 0;
        const bool upper    = g.below(4) == 0;
        const int  adjust   = int(g.below(4));
        long long  precision = (long long) g.below(25) - 2;
        if (g.below(30) == 0)
            precision = (long long) g.below(400);
        const long long width = g.below(3) == 0 ? (long long) g.below(40) : 0;
        const char      fill  = g.below(3) == 0 ? '*' : ' ';
        const int       loc   = g.below(10) == 0 ? 1 + int(g.below(2)) : 0;
        if (loc == 1)
            os.imbue(comma3);
        if (loc == 2)
            os.imbue(comma12);
        std::ios_base::fmtflags flags{};
        if (field == 1)
            flags |= std::ios_base::fixed;
        if (field == 2)
            flags |= std::ios_base::scientific;
        if (field == 3)
            flags |= std::ios_base::fixed | std::ios_base::scientific;
        if (showpos)
            flags |= std::ios_base::showpos;
        if (showpt)
            flags |= std::ios_base::showpoint;
        if (upper)
            flags |= std::ios_base::uppercase;
        if (adjust == 1)
            flags |= std::ios_base::left;
        if (adjust == 2)
            flags |= std::ios_base::internal;
        if (adjust == 3)
            flags |= std::ios_base::right;
        os.flags(flags);
        os.precision(precision);
        os.width(width);
        os.fill(fill);
        os << v;
        const bool  bad  = os.bad();
        std::string text = os.str();
        const uint8_t flag_bits = uint8_t((showpos ? 1 : 0) | (showpt ? 2 : 0) | (upper ? 4 : 0) | (bad ? 8 : 0));
        for (Payload *p : {&full, &committed}) {
            if (p == &committed && i >= CASES_COMMITTED)
                continue;
            p->u64(bits(v)).u8(uint8_t(field)).u8(flag_bits).u8(uint8_t(adjust)).i64(precision).i64(width).u8(uint8_t(fill)).u8(uint8_t(loc)).str(text);
        }
        to_string_cases.emplace_back(bits(v), std::to_string(v));
    }
    sets.full.add("OSTR", full);
    sets.committed.add("OSTR", committed);
    for (int which = 0; which < 2; ++which) {
        const size_t n = which == 0 ? CASES_COMMITTED : to_string_cases.size();
        Payload      p;
        p.u64(n);
        for (size_t i = 0; i < n; ++i)
            p.u64(to_string_cases[i].first).str(to_string_cases[i].second);
        (which == 0 ? sets.committed : sets.full).add("TOST", p);
    }
}

// ---------------------------------------------------------------- parsing

std::string random_number_text(SplitMix &g)
{
    static const char *const pieces[] = {"0", "1", "9", "5", "00", ".", "e", "E", "+", "-", "p", "P", "x", "X", "inf", "INFINITY", "nan", "nan(", ")", "ind",
                                         "snan", "a", "f", "F", " ", "\t", "z", "_", "12345678901234567890", "4", "7", "2", "3", "e-", "e+", "1e308",
                                         "1e-324", "0x", "0X1.", "ffff", "c", "d"};
    std::string s;
    const int   n = 1 + int(g.below(10));
    for (int i = 0; i < n; ++i)
        s += pieces[g.below(sizeof pieces / sizeof pieces[0])];
    return s;
}

std::string random_decimal_text(SplitMix &g)
{
    std::string s;
    if (g.below(4) == 0)
        s += '-';
    const int digits = 1 + int(g.below(g.below(10) == 0 ? 900 : 25));
    for (int i = 0; i < digits; ++i)
        s += char('0' + g.below(10));
    if (g.below(2))
        s.insert(g.below(s.size() + 1), ".");
    if (g.below(2)) {
        s += g.below(2) ? 'e' : 'E';
        if (g.below(2))
            s += g.below(2) ? '-' : '+';
        s += std::to_string(int(g.below(g.below(10) == 0 ? 6000 : 400)));
    }
    return s;
}

std::string random_hex_text(SplitMix &g)
{
    std::string s;
    if (g.below(4) == 0)
        s += '-';
    if (g.below(2))
        s += g.below(2) ? "0x" : "0X";
    const int        digits    = 1 + int(g.below(g.below(10) == 0 ? 300 : 20));
    static const char hexits[] = "0123456789abcdefABCDEF";
    for (int i = 0; i < digits; ++i)
        s += hexits[g.below(22)];
    if (g.below(2))
        s.insert(g.below(s.size() + 1), ".");
    if (g.below(2)) {
        s += g.below(2) ? 'p' : 'P';
        if (g.below(2))
            s += g.below(2) ? '-' : '+';
        s += std::to_string(int(g.below(2200)));
    }
    return s;
}

struct ParseCase
{
    std::string input;
    uint64_t    bits;
    int64_t     consumed;
    int32_t     status;
};

void add_parse(Sets &sets, const std::string &function, const std::vector<ParseCase> &cases)
{
    for (int which = 0; which < 2; ++which) {
        const size_t n = which == 0 ? Sets::committed_count(cases.size(), CASES_COMMITTED) : cases.size();
        Payload      p;
        p.str(function).u64(n);
        for (size_t i = 0; i < n; ++i)
            p.str(cases[i].input).u64(cases[i].bits).i64(cases[i].consumed).i32(cases[i].status);
        (which == 0 ? sets.committed : sets.full).add("PARS", p);
    }
}

void record_parsing(Sets &sets)
{
    SplitMix                 g(0x5041525300000001ull);
    std::vector<std::string> texts;
    char                     buf[2400];
    for (size_t i = 0; i < CASES_FULL; ++i) {
        switch (g.below(5)) {
        case 0: texts.push_back(random_number_text(g)); break;
        case 1:
        case 2: texts.push_back(random_decimal_text(g)); break;
        case 3: texts.push_back(random_hex_text(g)); break;
        default: {
            const double v = random_double(g);
            auto         r = std::to_chars(buf, buf + sizeof buf, v, g.below(2) ? std::chars_format::general : std::chars_format::hex);
            texts.emplace_back(buf, r.ptr);
        }
        }
    }
    std::vector<ParseCase> strtod_cases, strtof_cases, stod_cases, stof_cases;
    std::vector<ParseCase> fc_d[4], fc_f[4];
    for (const std::string &text : texts) {
        {
            errno        = 0;
            char *end    = nullptr;
            const double v = std::strtod(text.c_str(), &end);
            strtod_cases.push_back({text, bits(v), int64_t(end - text.c_str()), int32_t(errno)});
        }
        {
            errno       = 0;
            char *end   = nullptr;
            const float v = std::strtof(text.c_str(), &end);
            strtof_cases.push_back({text, bits(v), int64_t(end - text.c_str()), int32_t(errno)});
        }
        {
            double v      = 0;
            size_t idx    = 0;
            int    status = 0;
            try {
                v = std::stod(text, &idx);
            } catch (const std::invalid_argument &) {
                status = 1;
            } catch (const std::out_of_range &) {
                status = 2;
            }
            stod_cases.push_back({text, bits(v), int64_t(idx), status});
        }
        {
            float  v      = 0;
            size_t idx    = 0;
            int    status = 0;
            try {
                v = std::stof(text, &idx);
            } catch (const std::invalid_argument &) {
                status = 1;
            } catch (const std::out_of_range &) {
                status = 2;
            }
            stof_cases.push_back({text, bits(v), int64_t(idx), status});
        }
        for (int fi = 0; fi < 4; ++fi) {
            double d = from_bits(0xDEADBEEFull);
            auto   r = std::from_chars(text.data(), text.data() + text.size(), d, FORMATS[fi]);
            fc_d[fi].push_back({text, bits(d), int64_t(r.ptr - text.data()), errc_value(r.ec)});
            float f  = from_bits32(0xDEADBEEFu);
            auto  rf = std::from_chars(text.data(), text.data() + text.size(), f, FORMATS[fi]);
            fc_f[fi].push_back({text, bits(f), int64_t(rf.ptr - text.data()), errc_value(rf.ec)});
        }
    }
    add_parse(sets, "strtod", strtod_cases);
    add_parse(sets, "strtof", strtof_cases);
    add_parse(sets, "stod", stod_cases);
    add_parse(sets, "stof", stof_cases);
    const char *const names[] = {"scientific", "fixed", "hex", "general"};
    for (int fi = 0; fi < 4; ++fi) {
        add_parse(sets, std::string("from_chars-f64-") + names[fi], fc_d[fi]);
        add_parse(sets, std::string("from_chars-f32-") + names[fi], fc_f[fi]);
    }
}

} // namespace

void record_fmt(Sets &sets)
{
    head(sets, "fmt");
    record_printf(sets);
    record_to_chars(sets);
    record_ostream(sets);
    record_parsing(sets);
}

} // namespace probe
