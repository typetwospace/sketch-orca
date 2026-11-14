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
// The voronoi probe's records of voronoi-1's arithmetic (parity/sop1/voronoi.md, VRUL to VREC):
// Boost.Polygon's ulp_comparison, extended_exponent_fpt, extended_int, robust_fpt, robust_dif,
// robust_sqrt_expr, robust_cross_product with orientation_test, point_comparison_predicate and
// event_comparison_predicate, each called through its public interface on hand-made and seeded
// inputs. Cases whose results would hold a NaN are left out (their bits are the CPU's default NaN,
// which a port need not reproduce). Written by orca-o3-probe-voronoi, 2026-11-13.
#include "common.hpp"

#include <cfloat>
#include <cmath>
#include <cstring>
#include <initializer_list>
#include <limits>

namespace probe::vr {

namespace {

using Ulp   = bpd::ulp_comparison<double>;
using SqrtE = bpd::robust_sqrt_expr<BigI, Efpt, bpd::type_converter_efpt>;
using ECmp  = VP::event_comparison_predicate<Site, Circ>;
using PCmp  = VP::point_comparison_predicate<Pt>;
using OT    = VP::orientation_test;

const double kInf = std::numeric_limits<double>::infinity();

// ---------------------------------------------------------------- VRUL

void record_ulp(Sets &sets)
{
    Budget    b{120, 64};
    Ulp       cmp;
    auto      rec = [&](double x, double y, uint32_t ulps, bool hand) {
        Payload p;
        p.f64(x).f64(y).u32(ulps).i32(int32_t(cmp(x, y, ulps)));
        put_record(sets, "VRUL", p, b.take(p, hand));
    };
    const double specials[] = {0.0, -0.0, 1.0, -1.0, 0.5, 1.0000000000000002, 0.9999999999999999, 5e-324, -5e-324, 2.2250738585072014e-308,
                               DBL_MAX, -DBL_MAX, kInf, -kInf, from_bits(0x7FF8000000000000ull), from_bits(0xFFF8000000000000ull)};
    for (double x : specials)
        for (double y : specials)
            for (uint32_t u : {0u, 64u})
                rec(x, y, u, true);
    Gen g(0x0711Cu);
    for (int i = 0; i < 800; ++i) {
        const double x = g.chance(8) ? 0.0 : rand_double(g, -1022, 1023);
        double       y;
        if (g.chance(3)) {
            y = rand_double(g, -60, 60);
        } else {
            const int64_t k  = g.range(-300, 300);
            uint64_t      bx = bits(x);
            bx               = uint64_t(int64_t(bx) + k);
            y                = from_bits(bx);
            if (std::isnan(y))
                y = x;
        }
        const uint32_t pick[] = {0u, 1u, 64u, 128u, uint32_t(g.below(1000))};
        rec(x, y, pick[g.below(5)], false);
    }
}

// ---------------------------------------------------------------- VREF

bool finite_efpt(const Efpt &v)
{
    double val;
    std::memcpy(&val, reinterpret_cast<const char *>(&v), 8);
    return std::isfinite(val);
}

void record_efpt(Sets &sets)
{
    Budget b{90, 200};
    auto   rec = [&](double va, int32_t ea, double vb, int32_t eb, bool hand) {
        const Efpt A(va, ea), B(vb, eb), A1(va);
        const Efpt sum = A + B, dif = A - B, prod = A * B;
        if (!finite_efpt(sum) || !finite_efpt(dif) || !finite_efpt(prod))
            return;
        if (!B.is_zero() && !finite_efpt(A / B))
            return;
        Payload p;
        p.f64(va).i32(ea).f64(vb).i32(eb);
        put_efpt(p, A);
        put_efpt(p, B);
        put_efpt(p, A1);
        put_efpt(p, -A);
        put_efpt(p, sum);
        put_efpt(p, dif);
        put_efpt(p, prod);
        p.boolean(!B.is_zero());
        if (!B.is_zero())
            put_efpt(p, A / B);
        p.boolean(!A.is_neg());
        if (!A.is_neg())
            put_efpt(p, A.sqrt());
        p.f64(A.d()).f64(B.d());
        p.boolean(A.is_pos()).boolean(A.is_neg()).boolean(A.is_zero());
        put_record(sets, "VREF", p, b.take(p, hand));
    };
    // Hand-made: zeros, the exponent gap of 54 at its edges, subnormal values, cancellation.
    rec(0.0, 0, 0.0, 0, true);
    rec(1.0, 0, 0.0, 7, true);
    rec(0.0, 5, -3.0, -2, true);
    rec(1.0, 0, 1.0, 0, true);
    rec(1.0, 0, -1.0, 0, true);
    rec(0.75, 10, 0.75, 10 - 53, true);
    rec(0.75, 10, 0.75, 10 - 54, true);
    rec(0.75, 10, 0.75, 10 - 55, true);
    rec(0.75, 10 - 54, -0.75, 10, true);
    rec(0.75, 10 - 55, -0.75, 10, true);
    rec(5e-324, 0, 2.2250738585072014e-308, 0, true);
    rec(-3.0, 1, 2.0, -1, true);
    rec(2.0, 3, 0.5, -3, true);
    rec(123456789.0, -2000, 987654321.0, 2000, true);
    rec(1.5, 1, 1.5, 0, true);
    Gen g(0xEF97u);
    for (int i = 0; i < 700; ++i) {
        const double  va = g.chance(10) ? 0.0 : (g.chance(30) ? from_bits(g.next() & 0x000FFFFFFFFFFFFFull) : rand_double(g, -40, 40));
        const int32_t ea = int32_t(g.range(-1000, 1000));
        double        vb;
        int32_t       eb;
        if (g.chance(10)) {
            vb = -va;
            eb = ea;
        } else {
            vb = g.chance(10) ? 0.0 : rand_double(g, -40, 40);
            eb = g.chance(3) ? int32_t(ea + g.range(-57, 57)) : int32_t(g.range(-1000, 1000));
        }
        rec(va, ea, vb, eb, false);
    }
}

// ---------------------------------------------------------------- VRXC, VRXI

uint32_t rand_chunk(Gen &g)
{
    switch (g.below(8)) {
    case 0:
    case 1: return 0u;
    case 2:
    case 3: return 0xFFFFFFFFu;
    case 4: return 1u;
    default: return uint32_t(g.next());
    }
}

// A random extended_int of at most `max_chunks` chunks, its highest chunk not zero.
BigI rand_big(Gen &g, size_t max_chunks)
{
    const size_t          n = size_t(g.range(0, int64_t(max_chunks)));
    std::vector<uint32_t> c(n);
    for (uint32_t &x : c)
        x = rand_chunk(g);
    if (n != 0 && c[0] == 0)
        c[0] = 1u + uint32_t(g.below(0xFFFFFFFEull));
    return BigI(c, g.chance(2));
}

size_t rand_size(Gen &g)
{
    switch (g.below(10)) {
    case 0: return size_t(g.range(7, 40));
    case 1: return size_t(g.range(58, 64));
    default: return size_t(g.range(0, 6));
    }
}

void record_bigint_make(Sets &sets)
{
    Budget b{100, 300};
    auto   rec32 = [&](bpd::int32 v, bool hand) {
        Payload p;
        p.u8(0).i32(v);
        put_big(p, BigI(v));
        put_record(sets, "VRXC", p, b.take(p, hand));
    };
    auto rec64 = [&](bpd::int64 v, bool hand) {
        Payload p;
        p.u8(1).i64(v);
        put_big(p, BigI(v));
        put_record(sets, "VRXC", p, b.take(p, hand));
    };
    auto recv = [&](const std::vector<uint32_t> &c, bool plus, bool hand) {
        Payload p;
        p.u8(2).boolean(plus).u32(uint32_t(c.size()));
        for (uint32_t x : c)
            p.u32(x);
        put_big(p, BigI(c, plus));
        put_record(sets, "VRXC", p, b.take(p, hand));
    };
    for (bpd::int32 v : {0, 1, -1, 7, -7, 65536, 2147483647, -2147483647})
        rec32(v, true);
    for (bpd::int64 v : {0ll, 1ll, -1ll, 0xFFFFFFFFll, -0xFFFFFFFFll, 0x100000000ll, -0x100000000ll, 0x7FFFFFFFFFFFFFFFll, -0x7FFFFFFFFFFFFFFFll})
        rec64(v, true);
    recv({}, true, true);
    recv({}, false, true);
    recv({5}, false, true);
    recv({0, 5}, true, true);
    recv({1, 0, 0}, true, true);
    Gen g(0xB16Cu);
    for (size_t n : {63u, 64u, 65u, 70u}) {
        std::vector<uint32_t> c(n);
        for (uint32_t &x : c)
            x = uint32_t(g.next());
        recv(c, g.chance(2), true);
    }
    for (int i = 0; i < 80; ++i) {
        bpd::int32 v = bpd::int32(g.next());
        if (v == std::numeric_limits<bpd::int32>::min())
            v = 0;
        rec32(v, false);
        bpd::int64 w = bpd::int64(g.next() >> g.below(64));
        if (g.chance(2))
            w = -w;
        if (w == std::numeric_limits<bpd::int64>::min())
            w = 0;
        rec64(w, false);
        std::vector<uint32_t> c(rand_size(g));
        for (uint32_t &x : c)
            x = rand_chunk(g);
        recv(c, g.chance(2), false);
    }
}

void record_bigint_ops(Sets &sets)
{
    Budget b{100, 560};
    auto   rec = [&](const BigI &x, const BigI &y, bpd::int32 m, bpd::int64 m64, bool hand) {
        Payload p;
        put_big(p, x);
        put_big(p, y);
        p.u8(uint8_t((x == y ? 1 : 0) | (x != y ? 2 : 0) | (x < y ? 4 : 0) | (x > y ? 8 : 0) | (x <= y ? 16 : 0) | (x >= y ? 32 : 0)));
        put_big(p, x + y);
        put_big(p, x - y);
        put_big(p, x * y);
        put_big(p, -x);
        p.i32(m);
        put_big(p, x * m);
        p.i64(m64);
        put_big(p, x * m64);
        const std::pair<double, int> q = x.p();
        p.f64(q.first).i32(q.second).f64(x.d());
        put_efpt(p, bpd::type_converter_efpt()(x));
        p.f64(bpd::type_converter_fpt()(x));
        put_record(sets, "VRXI", p, b.take(p, hand));
    };
    auto big = [](std::vector<uint32_t> c, bool plus) { return BigI(c, plus); };
    // Hand-made: zeros, carries out of the top chunk, the difference whose top chunk is the only one
    // checked for zero (a non-normalized result), 64-chunk overflow.
    rec(big({}, true), big({}, true), 0, 0, true);
    rec(big({}, true), big({7}, false), 3, -3, true);
    rec(big({0xFFFFFFFFu}, true), big({1}, true), -1, 0x100000000ll, true);
    rec(big({1, 0, 0}, true), big({0xFFFFFFFFu, 0xFFFFFFFFu}, true), 2, 2, true);
    rec(big({1, 0}, true), big({1}, true), 1, 1, true);
    rec(big({1, 0}, false), big({1, 0}, false), 0, 0, true);
    rec(big({5, 0xFFFFFFFFu}, true), big({5, 0xFFFFFFFFu}, false), 7, -7, true);
    {
        std::vector<uint32_t> all(64, 0xFFFFFFFFu);
        rec(big(all, true), big(all, true), 2147483647, 0x7FFFFFFFFFFFFFFFll, true);
        rec(big(all, true), big({1}, true), -2147483647, -0x7FFFFFFFFFFFFFFFll, true);
    }
    Gen g(0x0B5u);
    for (int i = 0; i < 700; ++i) {
        const BigI x = rand_big(g, rand_size(g));
        const BigI y = g.chance(8) ? x : rand_big(g, rand_size(g));
        bpd::int32 m = bpd::int32(g.next() >> g.below(32));
        if (m == std::numeric_limits<bpd::int32>::min())
            m = 0;
        bpd::int64 m64 = bpd::int64(g.next() >> g.below(64));
        if (g.chance(2))
            m64 = -m64;
        if (m64 == std::numeric_limits<bpd::int64>::min())
            m64 = 0;
        rec(x, y, m, m64, false);
    }
}

// ---------------------------------------------------------------- VRRF, VRRD

bool any_nan(std::initializer_list<double> v)
{
    for (double x : v)
        if (std::isnan(x))
            return true;
    return false;
}

bool nan_rfpt(const Rfpt &v) { return std::isnan(v.fpv()) || std::isnan(v.re()); }
bool nan_rdif(const Rdif &v) { return nan_rfpt(v.pos()) || nan_rfpt(v.neg()); }

Rfpt rand_rfpt(Gen &g, bool nonneg)
{
    double v = g.chance(10) ? 0.0 : rand_double(g, -30, 30);
    if (nonneg)
        v = std::fabs(v);
    double re;
    switch (g.below(6)) {
    case 0: re = 0.0; break;
    case 1: re = 1.0; break;
    case 2: re = 2.0; break;
    case 3: re = 5.0; break;
    default: re = g.real(0., 40.);
    }
    return Rfpt(v, re);
}

void record_robust_fpt(Sets &sets)
{
    Budget b{70, 300};
    auto   rec = [&](const Rfpt &x, const Rfpt &y, bool hand) {
        std::vector<Rfpt> out;
        out.push_back(x + y);
        out.push_back(x - y);
        out.push_back(x * y);
        const bool div = y.fpv() != 0.0;
        if (div)
            out.push_back(x / y);
        Rfpt c = x;
        c += y;
        out.push_back(c);
        c = x;
        c -= y;
        out.push_back(c);
        c = x;
        c *= y;
        out.push_back(c);
        if (div) {
            c = x;
            c /= y;
            out.push_back(c);
        }
        out.push_back(-x);
        const bool root = !(x.fpv() < 0.0);
        if (root)
            out.push_back(x.sqrt());
        for (const Rfpt &r : out)
            if (nan_rfpt(r))
                return;
        Payload p;
        put_rfpt(p, x);
        put_rfpt(p, y);
        p.boolean(div).boolean(root);
        for (const Rfpt &r : out)
            put_rfpt(p, r);
        p.boolean(x.has_pos_value()).boolean(x.has_neg_value()).boolean(x.has_zero_value());
        put_record(sets, "VRRF", p, b.take(p, hand));
    };
    rec(Rfpt(0.0, 0.0), Rfpt(0.0, 1.0), true);
    rec(Rfpt(3.0, 1.0), Rfpt(3.0, 2.0), true);
    rec(Rfpt(3.0, 1.0), Rfpt(-3.0, 2.0), true);
    rec(Rfpt(-2.5, 4.0), Rfpt(1.25, 0.0), true);
    rec(Rfpt(1e300, 1.0), Rfpt(1e300, 1.0), true);
    rec(Rfpt(-0.0, 1.0), Rfpt(0.0, 1.0), true);
    rec(Rfpt(7.0), Rfpt(-7.0), true);
    Gen g(0x0FFFu);
    for (int i = 0; i < 600; ++i) {
        const Rfpt x = rand_rfpt(g, false);
        Rfpt       y = rand_rfpt(g, false);
        if (g.chance(8))
            y = Rfpt(-x.fpv(), y.re());
        else if (g.chance(8))
            y = Rfpt(x.fpv(), y.re());
        rec(x, y, false);
    }
}

void record_robust_dif(Sets &sets)
{
    Budget b{20, 800};
    auto   rec = [&](const Rdif &d1, const Rdif &d2, const Rfpt &v, bool hand) {
        std::vector<Rdif> out;
        out.push_back(-d1);
        out.push_back(d1 + d2);
        out.push_back(d1 + v);
        out.push_back(v + d1);
        out.push_back(d1 - d2);
        out.push_back(d1 - v);
        out.push_back(v - d1);
        out.push_back(d1 * d2);
        out.push_back(d1 * v);
        out.push_back(v * d1);
        const bool div = v.fpv() != 0.0;
        if (div)
            out.push_back(d1 / v);
        Rdif c = d1;
        c += v;
        out.push_back(c);
        c = d1;
        c += d2;
        out.push_back(c);
        c = d1;
        c -= v;
        out.push_back(c);
        c = d1;
        c -= d2;
        out.push_back(c);
        c = d1;
        c *= v;
        out.push_back(c);
        c = d1;
        c *= d2;
        out.push_back(c);
        if (div) {
            c = d1;
            c /= v;
            out.push_back(c);
        }
        const Rfpt dif = d1.dif();
        if (nan_rfpt(dif))
            return;
        for (const Rdif &r : out)
            if (nan_rdif(r))
                return;
        Payload p;
        put_rdif(p, d1);
        put_rdif(p, d2);
        put_rfpt(p, v);
        p.boolean(div);
        put_rfpt(p, dif);
        for (const Rdif &r : out)
            put_rdif(p, r);
        put_record(sets, "VRRD", p, b.take(p, hand));
    };
    rec(Rdif(), Rdif(), Rfpt(1.0, 1.0), true);
    rec(Rdif(Rfpt(2.0, 1.0), Rfpt(2.0, 1.0)), Rdif(Rfpt(1.0, 0.0), Rfpt(3.0, 2.0)), Rfpt(-4.0, 1.0), true);
    rec(Rdif(Rfpt(5.0, 3.0), Rfpt(0.0, 0.0)), Rdif(Rfpt(0.0, 0.0), Rfpt(5.0, 3.0)), Rfpt(0.0, 1.0), true);
    Gen g(0xD1Fu);
    for (int i = 0; i < 400; ++i) {
        const Rdif d1(rand_rfpt(g, true), rand_rfpt(g, true));
        const Rdif d2 = g.chance(8) ? d1 : Rdif(rand_rfpt(g, true), rand_rfpt(g, true));
        rec(d1, d2, rand_rfpt(g, false), false);
    }
}

// ---------------------------------------------------------------- VRSQ

void record_sqrt_expr(Sets &sets)
{
    Budget b{120, 220};
    auto   rec = [&](std::vector<BigI> A, std::vector<BigI> B, bool hand) {
        const size_t n = A.size();
        SqrtE        e;
        Efpt         r(0.0);
        switch (n) {
        case 1: r = e.eval1(A.data(), B.data()); break;
        case 2: r = e.eval2(A.data(), B.data()); break;
        case 3: r = e.eval3(A.data(), B.data()); break;
        default: r = e.eval4(A.data(), B.data()); break;
        }
        if (!finite_efpt(r))
            return;
        Payload p;
        p.u8(uint8_t(n));
        for (const BigI &a : A)
            put_big(p, a);
        for (const BigI &x : B)
            put_big(p, x);
        put_efpt(p, r);
        put_record(sets, "VRSQ", p, b.take(p, hand));
    };
    auto I = [](bpd::int64 v) { return BigI(v); };
    rec({I(3)}, {I(4)}, true);
    rec({I(-3)}, {I(0)}, true);
    rec({I(2), I(-3)}, {I(9), I(4)}, true);
    rec({I(2), I(-3)}, {I(10), I(4)}, true);
    rec({I(1), I(1), I(-1)}, {I(1), I(4), I(9)}, true);
    rec({I(5), I(-1), I(-1)}, {I(1), I(4), I(9)}, true);
    rec({I(1), I(1), I(-1), I(-1)}, {I(2), I(3), I(5), I(0)}, true);
    rec({I(7), I(-3), I(2), I(-5)}, {I(11), I(13), I(17), I(19)}, true);
    Gen g(0x5F2Eu);
    for (int i = 0; i < 600; ++i) {
        const size_t      n = size_t(g.range(1, 4));
        std::vector<BigI> A, B;
        for (size_t k = 0; k < n; ++k) {
            BigI a = rand_big(g, size_t(g.range(1, 3)));
            BigI x = rand_big(g, size_t(g.range(0, 3)));
            if (x.is_neg())
                x = -x;
            A.push_back(a);
            B.push_back(x);
        }
        if (n >= 2 && g.chance(3)) {
            // Exact cancellation of the first two terms: A0 sqrt(B0) = -A1 sqrt(B1).
            const bpd::int64 s = g.range(1, 100000), t = g.range(1, 100000), k = g.range(1, 100000);
            B[0] = BigI(bpd::int64(s * s));
            B[1] = BigI(bpd::int64(t * t));
            A[0] = BigI(bpd::int64(t * k));
            A[1] = BigI(bpd::int64(-s * k));
        }
        rec(A, B, false);
    }
}

// ---------------------------------------------------------------- VRCP, VRPC

void record_cross(Sets &sets)
{
    Budget b{100, 64};
    auto   rec4 = [&](bpd::int64 a1, bpd::int64 b1, bpd::int64 a2, bpd::int64 b2, bool hand) {
        Payload p;
        p.u8(0).i64(a1).i64(b1).i64(a2).i64(b2).f64(VP::robust_cross_product(a1, b1, a2, b2)).i32(int32_t(OT::eval(a1, b1, a2, b2)));
        put_record(sets, "VRCP", p, b.take(p, hand));
    };
    auto rec3 = [&](int32_t x1, int32_t y1, int32_t x2, int32_t y2, int32_t x3, int32_t y3, bool hand) {
        Payload p;
        p.u8(1).i32(x1).i32(y1).i32(x2).i32(y2).i32(x3).i32(y3).i32(int32_t(OT::eval(Pt(x1, y1), Pt(x2, y2), Pt(x3, y3))));
        put_record(sets, "VRCP", p, b.take(p, hand));
    };
    const bpd::int64 big = 0xFFFFFFFFll;
    rec4(0, 0, 0, 0, true);
    rec4(1, 2, 3, 4, true);
    rec4(-1, 2, 3, -4, true);
    rec4(big, big, big, big, true);
    rec4(big, -big, big, big, true);
    rec4(-big, big, -big, -big, true);
    rec4(big, big, -big, big, true);
    rec4(3, 6, 1, 2, true);
    rec3(0, 0, 1, 1, 2, 2, true);
    rec3(0, 0, 1, 0, 1, 1, true);
    rec3(0, 0, 1, 0, 1, -1, true);
    rec3(-2147483647, -2147483647, 2147483647, 2147483647, 2147483647, -2147483647, true);
    Gen g(0xC0CEu);
    for (int i = 0; i < 500; ++i) {
        auto pick = [&]() -> bpd::int64 {
            switch (g.below(3)) {
            case 0: return g.range(-10, 10);
            case 1: return g.range(-1000000, 1000000);
            default: return g.range(-big, big);
            }
        };
        rec4(pick(), pick(), pick(), pick(), false);
        auto c = [&]() -> int32_t { return g.chance(2) ? int32_t(g.range(-20, 20)) : int32_t(g.next()); };
        rec3(c(), c(), c(), c(), c(), c(), false);
    }
}

void record_point_cmp(Sets &sets)
{
    Budget     b{80, 32};
    PCmp       cmp;
    Gen        g(0x9C3Fu);
    for (int i = 0; i < 160; ++i) {
        const int32_t r = g.chance(2) ? 2 : 1000000;
        const Pt      x(int32_t(g.range(-r, r)), int32_t(g.range(-r, r)));
        const Pt      y = g.chance(5) ? x : Pt(int32_t(g.range(-r, r)), int32_t(g.range(-r, r)));
        Payload       p;
        p.i32(x.x()).i32(x.y()).i32(y.x()).i32(y.y()).boolean(cmp(x, y));
        put_record(sets, "VRPC", p, b.take(p, i < 12));
    }
}

// ---------------------------------------------------------------- VREC

Site rand_site(Gen &g, int32_t r, size_t index)
{
    const int32_t x0 = int32_t(g.range(-r, r)), y0 = int32_t(g.range(-r, r));
    if (g.chance(3))
        return mk_site(x0, y0, x0, y0, index, index, boost::polygon::SOURCE_CATEGORY_SINGLE_POINT, false);
    int32_t x1 = g.chance(4) ? x0 : int32_t(g.range(-r, r));
    int32_t y1 = int32_t(g.range(-r, r));
    if (x1 == x0 && y1 == y0)
        y1 = y0 + 1;
    // A segment site holds its smaller point first, as insert_segment makes it.
    const bool swap = (x1 < x0) || (x1 == x0 && y1 < y0);
    if (swap)
        return mk_site(x1, y1, x0, y0, index, index, boost::polygon::SOURCE_CATEGORY_REVERSE_SEGMENT, g.chance(4));
    return mk_site(x0, y0, x1, y1, index, index, boost::polygon::SOURCE_CATEGORY_INITIAL_SEGMENT, g.chance(4));
}

Circ rand_circle(Gen &g, double near_x)
{
    const double lower = g.chance(2) ? from_bits(uint64_t(int64_t(bits(near_x)) + g.range(-150, 150))) : near_x + g.real(-3., 3.);
    Circ         c(g.real(-5., 5.), g.chance(3) ? 0.5 : g.real(-5., 5.), std::isnan(lower) ? near_x : lower);
    if (g.chance(6))
        c.deactivate();
    return c;
}

void record_event_cmp(Sets &sets)
{
    Budget     b{120, 96};
    ECmp       cmp;
    Gen        g(0xEC0Cu);
    for (int i = 0; i < 1200; ++i) {
        const int32_t r    = g.chance(2) ? 3 : 1000;
        const int     form = int(g.below(4));
        Payload       p;
        p.u8(uint8_t(form));
        bool res = false;
        if (form == 0) {
            const Site x = rand_site(g, r, 0), y = rand_site(g, r, 1);
            put_site(p, x);
            put_site(p, y);
            res = cmp(x, y);
        } else if (form == 1) {
            const Site x = rand_site(g, r, 0);
            const Circ c = rand_circle(g, double(x.x0()));
            put_site(p, x);
            put_circle(p, c);
            res = cmp(x, c);
        } else if (form == 2) {
            const Site x = rand_site(g, r, 0);
            const Circ c = rand_circle(g, double(x.x0()));
            put_circle(p, c);
            put_site(p, x);
            res = cmp(c, x);
        } else {
            const Circ c1 = rand_circle(g, 1.0);
            Circ       c2 = rand_circle(g, 1.0);
            if (g.chance(3))
                c2 = Circ(c2.x(), c1.y(), c1.lower_x());
            put_circle(p, c1);
            put_circle(p, c2);
            res = cmp(c1, c2);
        }
        p.boolean(res);
        put_record(sets, "VREC", p, b.take(p, i < 16));
    }
}

} // namespace

void record_arith(Sets &sets)
{
    record_ulp(sets);
    record_efpt(sets);
    record_bigint_make(sets);
    record_bigint_ops(sets);
    record_robust_fpt(sets);
    record_robust_dif(sets);
    record_sqrt_expr(sets);
    record_cross(sets);
    record_point_cmp(sets);
    record_event_cmp(sets);
}

} // namespace probe::vr
