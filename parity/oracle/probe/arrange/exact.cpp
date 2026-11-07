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
// arrange.sop1's exact-arithmetic records (parity/SOP1.md, "arrange.sop1"), which orca-exact's
// tests/probes.rs replays: XI28 (Boost.Multiprecision's int128_t, the signed-magnitude 128-bit
// cpp_int Arrange.cpp names LargeInt under MSVC) and XRAT (boost::rational<int128_t>, the ratio
// libnest2d's nfpConvexOnly sorts edges with). Recorded by the o2-arrange task (2026-11-07). Each
// record holds its inputs and then Boost's results, so the replay needs no generator.
#include "../probe.hpp"

#include <cstdint>
#include <exception>
#include <functional>
#include <stdexcept>
#include <string>
#include <vector>

#include <boost/multiprecision/cpp_int.hpp>
#include <boost/multiprecision/integer.hpp>
#include <boost/rational.hpp>

namespace probe {

namespace {

using I128 = boost::multiprecision::int128_t;
using Rat  = boost::rational<I128>;

// A value from its sign and the two 64-bit halves of its magnitude.
I128 make(bool neg, uint64_t hi, uint64_t lo)
{
    I128 v = hi;
    v <<= 64;
    v |= I128(lo);
    if (neg)
        v = -v;
    return v;
}

// A value as SOP1 holds it: u8 negative, u64 high and u64 low half of the magnitude.
void put(Payload &p, const I128 &v)
{
    const bool neg = v < 0;
    const I128 m   = neg ? I128(-v) : v;
    const I128 hi  = m >> 64;
    const I128 lo  = m & I128(0xFFFFFFFFFFFFFFFFull);
    p.boolean(neg);
    p.u64(hi.convert_to<uint64_t>());
    p.u64(lo.convert_to<uint64_t>());
}

void put(Payload &p, const Rat &r)
{
    put(p, r.numerator());
    put(p, r.denominator());
}

void put(Payload &p, bool b) { p.boolean(b); }

// What a call that may throw gave: 0 and the value, or 1 and the exception's what(). True when
// the call returned.
template<class T, class F> bool outcome(Payload &p, F f)
{
    T           v{};
    bool        threw = false;
    std::string what;
    try {
        v = f();
    } catch (const std::exception &e) {
        threw = true;
        what  = e.what();
    }
    p.boolean(threw);
    if (threw)
        p.str(what);
    else
        put(p, v);
    return !threw;
}

// A magnitude of a chosen size: zero, one, a few bits, 32, 62, 64, 96 bits or all 128, or the
// largest; signed at random.
I128 rvalue(SplitMix &g)
{
    const uint64_t r   = g.next();
    const bool     neg = (g.next() & 1) != 0;
    switch (g.below(12)) {
    case 0: return I128(0);
    case 1: return make(neg, 0, 1);
    case 2: return make(neg, 0, r % 1000);
    case 3: return make(neg, 0, r & 0xFFFFFFFFull);
    case 4: return make(neg, 0, r & 0x3FFFFFFFFFFFFFFFull);
    case 5: return make(neg, 0, r);
    case 6: return make(neg, r & 0xFFFFFFFFull, g.next());
    case 7: return make(neg, r, g.next());
    case 8: return make(neg, ~uint64_t(0), ~uint64_t(0));
    case 9: return make(neg, 0, (r % 64) + 1);
    case 10: return make(neg, uint64_t(1) << (r % 64), 0);
    default: return make(neg, 0, uint64_t(1) << (r % 64));
    }
}

// One XI28 record: a and b, then a + b, a - b, a * b, a / b (which may throw), a % b when b is not
// zero (which may throw), -a, abs(a), gcd(a, b) (Boost.Multiprecision's, as boost::integer::gcd
// resolves to for int128_t; it may throw), double(a), a < b, a == b, a > b.
// - A zero b has no a % b: Boost 1.84's eval_modulus by a single limb (cpp_int/divide.hpp:549-570)
//   divides by it unchecked, a hardware integer divide by zero that ends the process (the
//   division itself throws).
// - gcd throws std::domain_error for some operands of four limbs: the Lehmer step of eval_gcd
//   (cpp_int/misc.hpp:670-858) multiplies U and V by its cosequences in 128 bits, so the products
//   wrap, U or V comes out negative or zero, and eval_lsb or eval_msb throws.
void xi28(Payload &p, const I128 &a, const I128 &b)
{
    put(p, a);
    put(p, b);
    put(p, I128(a + b));
    put(p, I128(a - b));
    put(p, I128(a * b));
    outcome<I128>(p, [&] { return I128(a / b); });
    if (b != 0)
        outcome<I128>(p, [&] { return I128(a % b); });
    put(p, I128(-a));
    put(p, I128(abs(a)));
    outcome<I128>(p, [&] { return I128(boost::integer::gcd(a, b)); });
    p.f64(static_cast<double>(a));
    p.boolean(a < b);
    p.boolean(a == b);
    p.boolean(a > b);
}

// One XRAT record: n1, d1, n2, d2 and i; then rational(n1, d1) and rational(n2, d2) (each may
// throw); when both exist: a + b, a - b, a * b, a / b, a < b, a > b, a == b, a < i, a > i, a == i,
// a + i, a - i, a * i, a / i, -a, abs(a) (each may throw) and rational_cast<double>(a).
void xrat(Payload &p, const I128 &n1, const I128 &d1, const I128 &n2, const I128 &d2, const I128 &i)
{
    put(p, n1);
    put(p, d1);
    put(p, n2);
    put(p, d2);
    put(p, i);
    Rat        a, b;
    const bool oka = outcome<Rat>(p, [&] {
        a = Rat(n1, d1);
        return a;
    });
    const bool okb = outcome<Rat>(p, [&] {
        b = Rat(n2, d2);
        return b;
    });
    if (!oka || !okb)
        return;
    outcome<Rat>(p, [&] { return Rat(a + b); });
    outcome<Rat>(p, [&] { return Rat(a - b); });
    outcome<Rat>(p, [&] { return Rat(a * b); });
    outcome<Rat>(p, [&] { return Rat(a / b); });
    outcome<bool>(p, [&] { return a < b; });
    outcome<bool>(p, [&] { return a > b; });
    outcome<bool>(p, [&] { return a == b; });
    outcome<bool>(p, [&] { return a < i; });
    outcome<bool>(p, [&] { return a > i; });
    outcome<bool>(p, [&] { return a == i; });
    outcome<Rat>(p, [&] { return Rat(a + i); });
    outcome<Rat>(p, [&] { return Rat(a - i); });
    outcome<Rat>(p, [&] { return Rat(a * i); });
    outcome<Rat>(p, [&] { return Rat(a / i); });
    outcome<Rat>(p, [&] { return Rat(-a); });
    outcome<Rat>(p, [&] { return Rat(abs(a)); });
    p.f64(boost::rational_cast<double>(a));
}

// The edge cases first (every one in both sets), then `random` generated ones, of which the
// committed set takes the first `committed`.
void record(Sets &sets, const char (&kind)[5], const std::vector<std::function<void(Payload &)>> &edges, size_t random, size_t committed,
            const std::function<void(Payload &)> &next)
{
    for (const auto &e : edges) {
        Payload p;
        e(p);
        sets.full.add(kind, p);
        sets.committed.add(kind, p);
    }
    for (size_t n = 0; n < random; ++n) {
        Payload p;
        next(p);
        sets.full.add(kind, p);
        if (n < committed)
            sets.committed.add(kind, p);
    }
}

} // namespace

void record_exact(Sets &sets)
{
    using Fn = std::function<void(Payload &)>;
    const uint64_t   all = ~uint64_t(0);
    const std::vector<I128> special{I128(0),
                                    make(false, 0, 1),
                                    make(true, 0, 1),
                                    make(false, 0, 2),
                                    make(true, 0, 3),
                                    make(false, 0, 7),
                                    make(false, 0, uint64_t(1) << 63),
                                    make(true, 0, uint64_t(1) << 63),
                                    make(false, 0, all),
                                    make(true, 0, all),
                                    make(false, 1, 0),
                                    make(true, 1, 0),
                                    make(false, uint64_t(1) << 63, 0),
                                    make(true, uint64_t(1) << 63, 0),
                                    make(false, all, all),
                                    make(true, all, all),
                                    make(false, all >> 1, all),
                                    make(false, 0, 1000000007)};

    std::vector<Fn> e;
    for (size_t x = 0; x < special.size(); x += 2)
        for (size_t y = 0; y < special.size(); y += 3)
            e.push_back([&special, x, y](Payload &p) { xi28(p, special[x], special[y]); });
    SplitMix g(0x58493238u);
    record(sets, "XI28", e, 4000, 120, [&g](Payload &p) {
        const I128 a = rvalue(g);
        const I128 b = rvalue(g);
        xi28(p, a, b);
    });

    e.clear();
    // The edge sort's own pattern, the zero denominators and the sign moves of normalize().
    e.push_back([](Payload &p) { xrat(p, I128(3), I128(4), I128(-5), I128(6), I128(1)); });
    e.push_back([](Payload &p) { xrat(p, I128(1), I128(0), I128(2), I128(3), I128(0)); });
    e.push_back([](Payload &p) { xrat(p, I128(2), I128(3), I128(0), I128(5), I128(0)); });
    e.push_back([](Payload &p) { xrat(p, I128(-6), I128(-4), I128(9), I128(-12), I128(-2)); });
    e.push_back([](Payload &p) { xrat(p, I128(0), I128(-7), I128(7), I128(7), I128(1)); });
    e.push_back([](Payload &p) {
        xrat(p, make(false, 0, uint64_t(1) << 31), make(false, 0, uint64_t(1) << 62), make(true, 0, (uint64_t(1) << 31) - 1),
             make(false, 0, (uint64_t(1) << 62) - 3), make(false, 0, uint64_t(1) << 31));
    });
    e.push_back([all](Payload &p) { xrat(p, make(false, all, all), make(false, 0, 3), make(true, all, all - 1), make(false, all, 1), make(false, all, all)); });
    SplitMix h(0x58524154u);
    record(sets, "XRAT", e, 2500, 60, [&h](Payload &p) {
        // Mostly the magnitudes the edge sort makes (a coordinate difference over a squared
        // length), now and then any 128-bit value.
        auto small = [&h]() -> I128 {
            const bool neg = (h.next() & 1) != 0;
            switch (h.below(5)) {
            case 0: return make(neg, 0, h.below(20));
            case 1: return make(neg, 0, h.next() & 0x7FFFFFFFull);
            case 2: return make(neg, 0, h.next() & 0x3FFFFFFFFFFFFFFFull);
            case 3: return make(neg, 0, h.below(1000) + 1);
            default: return rvalue(h);
            }
        };
        const I128 n1 = small(), d1 = small(), n2 = small(), d2 = small(), i = small();
        xrat(p, n1, d1, n2, d2, i);
    });
}

} // namespace probe
