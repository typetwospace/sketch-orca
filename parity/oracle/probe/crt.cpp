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
// crt.sop1: the static UCRT as the engine links it.
//
// - MATH: every CRT maths function of the oracle's linker map (parity probe passes them in), called
//   through a pointer so that the compiler can neither inline nor fold the call, over edge, hazard
//   and seeded random arguments, once with the UCRT's FMA3 paths on and once off
//   (_set_FMA3_enable). __vdecl_floor2, the SVML entry MSVC's auto-vectorizer calls for floor, is
//   called as the compiler calls it, two lanes in XMM0.
// - MATS: nan(const char *).
// - CAST: every float-to-integer conversion, static_cast<T>(double) and static_cast<T>(float) for
//   the eight integer types, as MSVC 14.42 compiles them in out-of-line functions.
// - RAND: rand() on a new thread, which starts from the UCRT's default seed, and after srand.
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <map>
#include <string>
#include <thread>
#include <vector>

#include <emmintrin.h>
#include <float.h>
#include <math.h>

#include "probe.hpp"

extern "C" void __vdecl_floor2();

namespace probe {

namespace {

// Cases per MATH record: the committed set keeps the first, the full set all.
constexpr size_t MATH_FULL      = 20000;
constexpr size_t MATH_COMMITTED = 384;
constexpr size_t CAST_FULL      = 20000;
constexpr size_t CAST_COMMITTED = 400;

// The argument kinds of a maths function, and its result kinds, as a signature: d double, f float,
// i a 32-bit int (int, long), l long long. "dd>d" is double f(double, double).
struct MathFn
{
    const char *name;
    const char *sig;
    void (*ptr)();
};

#define FN(name, sig, type) {#name, sig, reinterpret_cast<void (*)()>(static_cast<type>(&::name))}

using d_d   = double (*)(double);
using f_f   = float (*)(float);
using dd_d  = double (*)(double, double);
using ff_f  = float (*)(float, float);
using ddd_d = double (*)(double, double, double);
using d_i   = long (*)(double);
using d_fi  = int (*)(double);
using d_l   = long long (*)(double);
using d_di  = double (*)(double, int *);
using di_d  = double (*)(double, int);
using dld_d = double (*)(double, long double);
using v_v   = __m128d(__vectorcall *)(__m128d);

// Every maths function the probe knows how to call (and nan, which MATS records). record_crt
// refuses a maths symbol of the oracle's map that is not here (probe.ts crtMathsSymbols picks them
// out of the map), so a new maths call in the engine is probed before anything relies on it.
const std::vector<MathFn> &math_table()
{
    static const std::vector<MathFn> t = {
        FN(acos, "d>d", d_d),        FN(asin, "d>d", d_d),        FN(atan, "d>d", d_d),        FN(ceil, "d>d", d_d),
        FN(cos, "d>d", d_d),         FN(exp, "d>d", d_d),         FN(fabs, "d>d", d_d),        FN(floor, "d>d", d_d),
        FN(log, "d>d", d_d),         FN(log10, "d>d", d_d),       FN(nearbyint, "d>d", d_d),   FN(round, "d>d", d_d),
        FN(sin, "d>d", d_d),         FN(sqrt, "d>d", d_d),        FN(tan, "d>d", d_d),         FN(acosf, "f>f", f_f),
        FN(atanf, "f>f", f_f),       FN(ceilf, "f>f", f_f),       FN(cosf, "f>f", f_f),        FN(expf, "f>f", f_f),
        FN(floorf, "f>f", f_f),      FN(logf, "f>f", f_f),        FN(roundf, "f>f", f_f),      FN(sinf, "f>f", f_f),
        FN(sqrtf, "f>f", f_f),       FN(tanf, "f>f", f_f),        FN(truncf, "f>f", f_f),      FN(atan2, "dd>d", dd_d),
        FN(copysign, "dd>d", dd_d),  FN(_copysign, "dd>d", dd_d), FN(fmax, "dd>d", dd_d),      FN(fmod, "dd>d", dd_d),
        FN(hypot, "dd>d", dd_d),     FN(_hypot, "dd>d", dd_d),    FN(nextafter, "dd>d", dd_d), FN(pow, "dd>d", dd_d),
        FN(nexttoward, "dd>d", dld_d), FN(atan2f, "ff>f", ff_f),  FN(fmaxf, "ff>f", ff_f),     FN(fminf, "ff>f", ff_f),
        FN(fmodf, "ff>f", ff_f),     FN(powf, "ff>f", ff_f),      FN(fma, "ddd>d", ddd_d),     FN(llround, "d>l", d_l),
        FN(lround, "d>i", d_i),      FN(lrint, "d>i", d_i),       FN(_finite, "d>i", d_fi),    FN(frexp, "d>di", d_di),
        FN(ldexp, "di>d", di_d),       FN(_isnan, "d>i", d_fi),     FN(_fpclass, "d>i", d_fi),
        {"__vdecl_floor2", "dd>dd", &__vdecl_floor2},
    };
    return t;
}

#undef FN

// Doubles: specials, edges and hazards, then seeded random values of several shapes.
const uint64_t SPECIAL_D[] = {
    0x0000000000000000ull, 0x8000000000000000ull, 0x7FF0000000000000ull, 0xFFF0000000000000ull, 0x7FF8000000000000ull,
    0xFFF8000000000000ull, 0x7FF0000000000001ull, 0x7FF4000000000000ull, 0x7FF8000000000001ull, 0x0000000000000001ull,
    0x8000000000000001ull, 0x000FFFFFFFFFFFFFull, 0x0010000000000000ull, 0x7FEFFFFFFFFFFFFFull, 0x3FF0000000000000ull,
    0xBFF0000000000000ull, 0x3FE0000000000000ull, 0xBFE0000000000000ull, 0x3FEFFFFFFFFFFFFFull, 0x3FF0000000000001ull,
    0x4000000000000000ull, 0x400921FB54442D18ull, 0x3FF921FB54442D18ull, 0x3FE921FB54442D18ull, 0x401921FB54442D18ull,
    0xC00921FB54442D18ull, 0x4005BF0A8B145769ull, 0x3FB999999999999Aull, 0x4340000000000000ull, 0x4340000000000001ull,
    0x433FFFFFFFFFFFFFull, 0x4330000000000000ull, 0x44B52D02C7E14AF6ull, 0x40862E42FEFA39EFull, 0xC0874910D52D3051ull,
    0x40862E42FEFA39F0ull, 0x3E50000000000000ull, 0x3C90000000000000ull, 0x4415AF1D78B58C40ull, 0x7E37E43C8800759Cull,
    0x41DFFFFFFFC00000ull, 0xC1E0000000000000ull, 0x43E0000000000000ull, 0xC3E0000000000000ull, 0x3FF8000000000000ull,
    0xBFF8000000000000ull, 0x4004000000000000ull, 0xC004000000000000ull,
};

double random_double(SplitMix &g)
{
    switch (g.below(9)) {
    case 0: return from_bits(SPECIAL_D[g.below(sizeof SPECIAL_D / sizeof SPECIAL_D[0])]);
    case 1:
    case 2: return from_bits(g.next());
    case 3: return double(int64_t(g.next()) >> g.below(64)) / double(1ull << g.below(63));
    case 4: return (double(g.next() >> 11) / 9007199254740992.0) * 2.0 - 1.0;
    case 5: return ((double(g.next() >> 11) / 9007199254740992.0) * 2.0 - 1.0) * 10.0;
    case 6: return ((double(g.next() >> 11) / 9007199254740992.0) * 2.0 - 1.0) * 1000.0;
    case 7: return double(int64_t(g.below(2001)) - 1000) * 0.5;
    default: return std::ldexp(double(g.next() >> 11), int(g.below(2100)) - 1100);
    }
}

float random_float(SplitMix &g)
{
    switch (g.below(6)) {
    case 0: return float(from_bits(SPECIAL_D[g.below(sizeof SPECIAL_D / sizeof SPECIAL_D[0])]));
    case 1:
    case 2: return from_bits32(uint32_t(g.next()));
    case 3: return float(((double(g.next() >> 11) / 9007199254740992.0) * 2.0 - 1.0) * 10.0);
    case 4: return float(double(int64_t(g.below(2001)) - 1000) * 0.5);
    default: return float(random_double(g));
    }
}

int random_int(SplitMix &g)
{
    switch (g.below(4)) {
    case 0: return int(g.below(41)) - 20;
    case 1: return int(g.below(4401)) - 2200;
    case 2: return int(uint32_t(g.next()));
    default: {
        const int edges[] = {0, 1, -1, 1023, 1024, -1022, -1074, -1075, 2047, -2098, 2147483647, int(0x80000000u)};
        return edges[g.below(sizeof edges / sizeof edges[0])];
    }
    }
}

uint64_t value_bits(char kind, SplitMix &g)
{
    switch (kind) {
    case 'd': return bits(random_double(g));
    case 'f': return bits(random_float(g));
    default: return uint64_t(int64_t(random_int(g)));
    }
}

// The arguments of one call, as bits. The first cases run every special against every special
// (for one argument each special once) so that the committed set holds the edges.
std::vector<std::vector<uint64_t>> math_arguments(const std::string &args, size_t count, uint64_t seed)
{
    std::vector<std::vector<uint64_t>> out;
    const size_t specials = sizeof SPECIAL_D / sizeof SPECIAL_D[0];
    const auto special = [&](char kind, size_t i) -> uint64_t {
        const double d = from_bits(SPECIAL_D[i % specials]);
        if (kind == 'd')
            return bits(d);
        if (kind == 'f')
            return bits(float(d));
        const int ints[] = {0, 1, -1, 2, -2, 10, 52, 53, 1023, 1024, -1022, -1074, -1075, 2147483647, int(0x80000000u)};
        return uint64_t(int64_t(ints[i % (sizeof ints / sizeof ints[0])]));
    };
    if (args.size() == 1) {
        for (size_t i = 0; i < specials && out.size() < count; ++i)
            out.push_back({special(args[0], i)});
    } else {
        for (size_t i = 0; i < specials && out.size() < count; ++i)
            for (size_t j = 0; j < specials && out.size() < count; j += 5) {
                std::vector<uint64_t> a;
                for (size_t k = 0; k < args.size(); ++k)
                    a.push_back(special(args[k], k == 0 ? i : j + k));
                out.push_back(a);
            }
    }
    SplitMix g(seed);
    while (out.size() < count) {
        std::vector<uint64_t> a;
        for (char kind : args)
            a.push_back(value_bits(kind, g));
        out.push_back(a);
    }
    return out;
}

uint64_t name_seed(const std::string &name)
{
    uint64_t h = 1469598103934665603ull;
    for (unsigned char c : name) {
        h ^= c;
        h *= 1099511628211ull;
    }
    return h;
}

// One call of `fn` on `a` (bits), its results as bits.
std::vector<uint64_t> call(const MathFn &fn, const std::vector<uint64_t> &a)
{
    const std::string sig = fn.sig;
    const auto d  = [&](size_t i) { return from_bits(a[i]); };
    const auto f  = [&](size_t i) { return from_bits32(uint32_t(a[i])); };
    const auto in = [&](size_t i) { return int(int64_t(a[i])); };
    if (sig == "d>d")
        return {bits(reinterpret_cast<d_d>(fn.ptr)(d(0)))};
    if (sig == "f>f")
        return {uint64_t(bits(reinterpret_cast<f_f>(fn.ptr)(f(0))))};
    if (sig == "dd>d") {
        if (std::strcmp(fn.name, "nexttoward") == 0)
            return {bits(reinterpret_cast<dld_d>(fn.ptr)(d(0), static_cast<long double>(d(1))))};
        return {bits(reinterpret_cast<dd_d>(fn.ptr)(d(0), d(1)))};
    }
    if (sig == "ff>f")
        return {uint64_t(bits(reinterpret_cast<ff_f>(fn.ptr)(f(0), f(1))))};
    if (sig == "ddd>d")
        return {bits(reinterpret_cast<ddd_d>(fn.ptr)(d(0), d(1), d(2)))};
    if (sig == "d>l")
        return {uint64_t(reinterpret_cast<d_l>(fn.ptr)(d(0)))};
    if (sig == "d>i") {
        if (fn.name[0] == '_') // _finite, _isnan and _fpclass return int, lround and lrint long.
            return {uint64_t(int64_t(reinterpret_cast<d_fi>(fn.ptr)(d(0))))};
        return {uint64_t(int64_t(reinterpret_cast<d_i>(fn.ptr)(d(0))))};
    }
    if (sig == "d>di") {
        int        e = 0x7EADBEEF;
        const double m = reinterpret_cast<d_di>(fn.ptr)(d(0), &e);
        return {bits(m), uint64_t(int64_t(e))};
    }
    if (sig == "di>d")
        return {bits(reinterpret_cast<di_d>(fn.ptr)(d(0), in(1)))};
    if (sig == "dd>dd") {
        const __m128d r = reinterpret_cast<v_v>(fn.ptr)(_mm_set_pd(d(1), d(0)));
        double lanes[2];
        _mm_storeu_pd(lanes, r);
        return {bits(lanes[0]), bits(lanes[1])};
    }
    return {};
}

// The argument part of a signature: "dd>d" -> "dd"; "dd>dd" (two lanes) -> "dd".
std::string arg_kinds(const char *sig) { return std::string(sig, std::strchr(sig, '>')); }

void record_math(Sets &sets, const MathFn &fn)
{
    const std::string kinds = arg_kinds(fn.sig);
    const auto        args  = math_arguments(kinds, MATH_FULL, name_seed(fn.name));
    for (int fma3 : {1, 0}) {
        _set_FMA3_enable(fma3);
        const int state = _get_FMA3_enable();
        std::vector<std::vector<uint64_t>> results;
        results.reserve(args.size());
        for (const auto &a : args)
            results.push_back(call(fn, a));
        for (int which = 0; which < 2; ++which) {
            const size_t n = which == 0 ? Sets::committed_count(args.size(), MATH_COMMITTED) : args.size();
            Payload      p;
            p.str(fn.name).i32(state).str(fn.sig).u64(n);
            for (size_t i = 0; i < n; ++i) {
                for (uint64_t v : args[i])
                    p.u64(v);
                for (uint64_t v : results[i])
                    p.u64(v);
            }
            (which == 0 ? sets.committed : sets.full).add("MATH", p);
        }
    }
    _set_FMA3_enable(1);
}

void record_nan(Sets &sets)
{
    const char *const inputs[] = {"", "0", "1", "-1", "123", "0x10", "0X7ffffffffffff", "4503599627370495", "4503599627370496",
                                  "abc", "ind", "snan", "inf", "1e5", "   7", "18446744073709551615", "nan", "()", "0x", "08"};
    for (int fma3 : {1, 0}) {
        _set_FMA3_enable(fma3);
        const int state = _get_FMA3_enable();
        double (*volatile fn)(const char *) = &::nan;
        Payload p;
        p.str("nan").i32(state).u64(sizeof inputs / sizeof inputs[0]);
        for (const char *s : inputs)
            p.str(s).u64(bits(fn(s)));
        sets.committed.add("MATS", p);
        sets.full.add("MATS", p);
    }
    _set_FMA3_enable(1);
}

// The conversions, out of line so each is compiled as the engine compiles a cast.
#define CASTS(SUFFIX, F)                                                                                         \
    __declspec(noinline) signed char to_i8##SUFFIX(F x) { return static_cast<signed char>(x); }                 \
    __declspec(noinline) unsigned char to_u8##SUFFIX(F x) { return static_cast<unsigned char>(x); }             \
    __declspec(noinline) short to_i16##SUFFIX(F x) { return static_cast<short>(x); }                            \
    __declspec(noinline) unsigned short to_u16##SUFFIX(F x) { return static_cast<unsigned short>(x); }          \
    __declspec(noinline) int to_i32##SUFFIX(F x) { return static_cast<int>(x); }                                \
    __declspec(noinline) unsigned to_u32##SUFFIX(F x) { return static_cast<unsigned>(x); }                      \
    __declspec(noinline) long long to_i64##SUFFIX(F x) { return static_cast<long long>(x); }                    \
    __declspec(noinline) unsigned long long to_u64##SUFFIX(F x) { return static_cast<unsigned long long>(x); }
CASTS(_d, double)
CASTS(_f, float)
#undef CASTS

std::vector<double> cast_inputs(SplitMix &g)
{
    std::vector<double> ds = {0.0, -0.0, 0.5, -0.5, 0.999, -0.999, 1.0, -1.0, 127.0, 127.9, 128.0, -128.0, -128.9, -129.0, 255.0, 255.9,
                              256.0, 32767.0, 32768.0, -32768.0, -32769.0, 65535.0, 65536.0, 70000.0, -70000.0, 2147483647.0, 2147483647.5,
                              2147483648.0, -2147483648.0, -2147483648.5, -2147483649.0, 4294967295.0, 4294967295.5, 4294967296.0, 1e10,
                              -1e10, 9223372036854774784.0, 9223372036854775808.0, -9223372036854775808.0, -9223372036854777856.0,
                              18446744073709549568.0, 18446744073709551616.0, 36893488147419103232.0, 1e19, 1e20, 1e30, -1e30, 1e308,
                              -1e308, 5e-324, -5e-324};
    for (uint64_t b : {0x7FF0000000000000ull, 0xFFF0000000000000ull, 0x7FF8000000000000ull, 0xFFF8000000000000ull, 0x7FF0000000000001ull})
        ds.push_back(from_bits(b));
    while (ds.size() < CAST_FULL) {
        double v = random_double(g);
        if (g.below(2))
            v = std::ldexp(double(int64_t(g.next())), -int(g.below(70)));
        ds.push_back(v);
    }
    return ds;
}

template<class F> void record_casts(Sets &sets, const char *from, const std::vector<F> &inputs)
{
    struct Cast
    {
        const char *to;
        uint64_t (*fn)(F);
    };
    const bool is_double = sizeof(F) == 8;
    const Cast casts[]   = {
        {"i8", [](F x) { return uint64_t(int64_t(sizeof(F) == 8 ? to_i8_d(double(x)) : to_i8_f(float(x)))); }},
        {"u8", [](F x) { return uint64_t(sizeof(F) == 8 ? to_u8_d(double(x)) : to_u8_f(float(x))); }},
        {"i16", [](F x) { return uint64_t(int64_t(sizeof(F) == 8 ? to_i16_d(double(x)) : to_i16_f(float(x)))); }},
        {"u16", [](F x) { return uint64_t(sizeof(F) == 8 ? to_u16_d(double(x)) : to_u16_f(float(x))); }},
        {"i32", [](F x) { return uint64_t(int64_t(sizeof(F) == 8 ? to_i32_d(double(x)) : to_i32_f(float(x)))); }},
        {"u32", [](F x) { return uint64_t(sizeof(F) == 8 ? to_u32_d(double(x)) : to_u32_f(float(x))); }},
        {"i64", [](F x) { return uint64_t(sizeof(F) == 8 ? to_i64_d(double(x)) : to_i64_f(float(x))); }},
        {"u64", [](F x) { return uint64_t(sizeof(F) == 8 ? to_u64_d(double(x)) : to_u64_f(float(x))); }},
    };
    (void) is_double;
    for (const Cast &c : casts) {
        uint64_t (*volatile fn)(F) = c.fn;
        for (int which = 0; which < 2; ++which) {
            const size_t n = which == 0 ? Sets::committed_count(inputs.size(), CAST_COMMITTED) : inputs.size();
            Payload      p;
            p.str(std::string(from) + ">" + c.to).u64(n);
            for (size_t i = 0; i < n; ++i)
                p.u64(sizeof(F) == 8 ? bits(double(inputs[i])) : uint64_t(bits(float(inputs[i])))).u64(fn(inputs[i]));
            (which == 0 ? sets.committed : sets.full).add("CAST", p);
        }
    }
}

void record_rand(Sets &sets)
{
    const auto add = [&](const char *what, uint32_t seed, const std::vector<int32_t> &values) {
        Payload p;
        p.str(what).u32(seed).i32s(values);
        sets.committed.add("RAND", p);
        sets.full.add("RAND", p);
    };
    std::vector<int32_t> fresh;
    std::thread          t([&fresh] {
        for (int i = 0; i < 64; ++i)
            fresh.push_back(std::rand());
    });
    t.join();
    add("fresh-thread", 0, fresh);
    for (uint32_t seed : {0u, 1u, 2u, 42u, 12345u, 0x7FFFFFFFu, 0x80000000u, 0xFFFFFFFFu}) {
        std::vector<int32_t> v;
        std::thread          s([&v, seed] {
            std::srand(seed);
            for (int i = 0; i < 64; ++i)
                v.push_back(std::rand());
        });
        s.join();
        add("srand", seed, v);
    }
    // srand on one thread leaves another thread's sequence where it was: the state is per thread.
    std::vector<int32_t> other;
    std::thread          u([&other] {
        other.push_back(std::rand());
        std::thread w([] { std::srand(99); });
        w.join();
        for (int i = 0; i < 7; ++i)
            other.push_back(std::rand());
    });
    u.join();
    add("per-thread", 99, other);
}

} // namespace

bool record_crt(Sets &sets, const std::vector<std::string> &crt_symbols, std::string &error)
{
    head(sets, "crt");
    std::map<std::string, const MathFn *> known;
    for (const MathFn &fn : math_table())
        known[fn.name] = &fn;
    for (const std::string &s : crt_symbols) {
        if (s == "nan")
            continue;
        const auto it = known.find(s);
        if (it == known.end()) {
            error = "the oracle's map lists the CRT maths function " + s + ", which the probe does not know (crt.cpp math_table)";
            return false;
        }
        record_math(sets, *it->second);
    }
    bool has_nan = false;
    for (const std::string &s : crt_symbols)
        has_nan = has_nan || s == "nan";
    if (has_nan)
        record_nan(sets);
    SplitMix            g(0x0DDBA11ull);
    std::vector<double> ds = cast_inputs(g);
    std::vector<float>  fs;
    for (double d : ds)
        fs.push_back(g.below(3) == 0 ? from_bits32(uint32_t(g.next())) : float(d));
    record_casts(sets, "f64", ds);
    record_casts(sets, "f32", fs);
    record_rand(sets);
    return true;
}

namespace {

bool run_crt(const Options &options, std::string &error)
{
    Sets sets;
    return record_crt(sets, options.crt_symbols, error) && save(sets, options, "crt.sop1", error);
}

} // namespace

const Target &target()
{
    static const Target t{"crt", true, run_crt};
    return t;
}

} // namespace probe
