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
// geom.sop1's Eigen records (parity/SOP1.md, "geom.sop1"): what the engine's Eigen 5.0.1, compiled
// with its own flags, computes for libslic3r's fixed-size types, which orca-eigen's
// tests/probes.rs replays (added 2026-11-07 by o2-geom):
//
//   EVEC  vector operations of Vec2/3/4 of double, float, int64 and int32
//   EMAT  products, determinant, inverse and reductions of 2x2, 3x3 and 4x4 matrices
//   ETRF  Transform3d built as Geometry::assemble_transform builds it, its products, inverse,
//         computeRotationScaling and the Euler angles of its rotation, and Transform3f
//   EROT  AngleAxis and Quaternion conversions and products, eulerAngles
//   ESVD  JacobiSVD<Matrix3d> with full U and V
//
// Inputs come from the shared SplitMix generator: wide magnitudes (so that the order of additions
// shows in the last bits), small integers, signed zeros, and in EVEC infinities and one NaN (the
// default quiet NaN only: two NaNs of different bits would make the result depend on which operand
// MSVC puts first).
#include "../probe.hpp"

#include <cmath>
#include <cstdint>
#include <limits>

#include <Eigen/Dense>
#include <Eigen/Geometry>

namespace probe {

namespace {

using V2d = Eigen::Matrix<double, 2, 1, Eigen::DontAlign>;
using V3d = Eigen::Matrix<double, 3, 1, Eigen::DontAlign>;
using V4d = Eigen::Matrix<double, 4, 1, Eigen::DontAlign>;
using M3d = Eigen::Matrix<double, 3, 3, Eigen::DontAlign>;
using M3f = Eigen::Matrix<float, 3, 3, Eigen::DontAlign>;
using V3f = Eigen::Matrix<float, 3, 1, Eigen::DontAlign>;
using T3d = Eigen::Transform<double, 3, Eigen::Affine, Eigen::DontAlign>;
using T3f = Eigen::Transform<float, 3, Eigen::Affine, Eigen::DontAlign>;

const double QNAN = from_bits(0xFFF8000000000000ull);

// A double of wide magnitude, a small integer, a signed zero or, when `specials`, an infinity or
// the default NaN.
double gen(SplitMix &g, bool specials)
{
    const uint64_t r = g.below(32);
    if (r == 0)
        return 0.0;
    if (r == 1)
        return -0.0;
    if (r < 5)
        return double(int64_t(g.below(41)) - 20);
    if (specials && r == 5)
        return (g.next() & 1) ? std::numeric_limits<double>::infinity() : -std::numeric_limits<double>::infinity();
    if (specials && r == 6)
        return QNAN;
    const double m = 1.0 + double(g.next() >> 11) * 0x1.0p-53;
    const int    e = int(g.below(61)) - 30;
    const double v = std::ldexp(m, e);
    return (g.next() & 1) ? -v : v;
}

uint64_t b(double v) { return bits(v); }
uint64_t b(float v) { return bits(v); }
uint64_t b(int64_t v) { return uint64_t(v); }
uint64_t b(int32_t v) { return uint64_t(int64_t(v)); }

template<class D> void put(Payload &p, const Eigen::DenseBase<D> &m)
{
    for (Eigen::Index c = 0; c < m.cols(); ++c)
        for (Eigen::Index r = 0; r < m.rows(); ++r)
            p.u64(b(typename D::Scalar(m(r, c))));
}

template<class T> T cast_in(double v);
template<> double  cast_in<double>(double v) { return v; }
template<> float   cast_in<float>(double v) { return float(v); }
template<> int64_t cast_in<int64_t>(double v) { return std::isfinite(v) && std::fabs(v) < 9e18 ? int64_t(v) : 7; }
template<> int32_t cast_in<int32_t>(double v) { return std::isfinite(v) && std::fabs(v) < 2e9 ? int32_t(v) : 7; }

template<class T, int N> void evec_case(Payload &p, SplitMix &g, uint8_t type)
{
    using V = Eigen::Matrix<T, N, 1, Eigen::DontAlign>;
    V a, c;
    for (int i = 0; i < N; ++i) {
        a(i) = cast_in<T>(gen(g, std::is_floating_point<T>::value));
        c(i) = cast_in<T>(gen(g, std::is_floating_point<T>::value));
    }
    p.u8(type).u8(uint8_t(N));
    put(p, a);
    put(p, c);
    p.u64(b(T(a.dot(c))));
    p.u64(b(T(a.squaredNorm())));
    p.u64(b(T(a.sum())));
    p.u64(b(T(a.prod())));
    p.u64(b(T(a.minCoeff())));
    p.u64(b(T(a.maxCoeff())));
    p.u64(b(T((a - c).squaredNorm())));
    Eigen::Index imin, imax;
    a.minCoeff(&imin);
    a.maxCoeff(&imax);
    p.u64(uint64_t(imin)).u64(uint64_t(imax));
    put(p, a.cwiseMin(c));
    put(p, a.cwiseMax(c));
    put(p, a.cwiseAbs());
    put(p, a.cwiseProduct(c));
    put(p, (a + c).eval());
    put(p, (a - c).eval());
    if constexpr (std::is_floating_point<T>::value) {
        p.u64(b(T(a.norm())));
        put(p, a.normalized());
        p.u64(a.isApprox(c, T(1e-3)) ? 1 : 0);
        put(p, (a * T(0.375)).eval());
        put(p, (a / T(3)).eval());
    }
    if constexpr (N == 3)
        put(p, a.cross(c));
    if constexpr (N == 2)
        p.u64(b(T(a.cross(c))));
    put(p, a.template cast<double>());
    put(p, a.template cast<float>());
    put(p, a.template cast<int64_t>());
    put(p, a.template cast<int32_t>());
}

template<class T, int N> void record_evec(Sets &sets, SplitMix &g, uint8_t type, size_t full, size_t committed)
{
    for (size_t i = 0; i < full; ++i) {
        Payload p;
        evec_case<T, N>(p, g, type);
        sets.full.add("EVEC", p);
        if (i < committed)
            sets.committed.add("EVEC", p);
    }
}

template<class T, int N> void emat_case(Payload &p, SplitMix &g, uint8_t type)
{
    using M = Eigen::Matrix<T, N, N, Eigen::DontAlign>;
    using V = Eigen::Matrix<T, N, 1, Eigen::DontAlign>;
    M a, c;
    V v;
    for (int j = 0; j < N; ++j)
        for (int i = 0; i < N; ++i) {
            a(i, j) = T(gen(g, false));
            c(i, j) = T(gen(g, false));
        }
    for (int i = 0; i < N; ++i)
        v(i) = T(gen(g, false));
    p.u8(type).u8(uint8_t(N));
    put(p, a);
    put(p, c);
    put(p, v);
    M ac = a * c;
    put(p, ac);
    V av = a * v;
    put(p, av);
    M act = a * c.transpose();
    put(p, act);
    V nav = -a * v;
    put(p, nav);
    M ad = a * v.asDiagonal();
    put(p, ad);
    M da = v.asDiagonal() * a;
    put(p, da);
    p.u64(b(T(a.sum())));
    p.u64(b(T(a.squaredNorm())));
    p.u64(b(T(a.trace())));
    p.u64(a.isDiagonal() ? 1 : 0);
    if constexpr (N == 3) {
        p.u64(b(T(a.determinant())));
        M inv = a.inverse();
        put(p, inv);
    }
}

template<class T, int N> void record_emat(Sets &sets, SplitMix &g, uint8_t type, size_t full, size_t committed)
{
    for (size_t i = 0; i < full; ++i) {
        Payload p;
        emat_case<T, N>(p, g, type);
        sets.full.add("EMAT", p);
        if (i < committed)
            sets.committed.add("EMAT", p);
    }
}

// An angle in radians: wide, a multiple of a quarter turn, or zero.
double gen_angle(SplitMix &g)
{
    const uint64_t r = g.below(8);
    if (r == 0)
        return 0.0;
    if (r == 1)
        return double(int64_t(g.below(9)) - 4) * 0.5 * EIGEN_PI;
    return (double(g.next() >> 11) * 0x1.0p-53 * 4.0 - 2.0) * EIGEN_PI;
}

// A scale factor: positive, sometimes negative (a mirror), sometimes 1.
double gen_scale(SplitMix &g)
{
    const uint64_t r = g.below(6);
    if (r == 0)
        return 1.0;
    const double s = std::ldexp(1.0 + double(g.next() >> 11) * 0x1.0p-53, int(g.below(9)) - 4);
    return r == 1 ? -s : s;
}

V3d gen_v3(SplitMix &g)
{
    return V3d(gen(g, false), gen(g, false), gen(g, false));
}

// Geometry::assemble_transform: translate, rotate by Z * Y * X angle-axes, scale.
T3d assemble(const V3d &t, const V3d &r, const V3d &s)
{
    T3d tr = T3d::Identity();
    tr.translate(t);
    tr.rotate(Eigen::AngleAxisd(r(2), V3d::UnitZ()) * Eigen::AngleAxisd(r(1), V3d::UnitY()) * Eigen::AngleAxisd(r(0), V3d::UnitX()));
    tr.scale(s);
    return tr;
}

void etrf_case(Payload &p, SplitMix &g)
{
    const V3d t1 = gen_v3(g), t2 = gen_v3(g), pt = gen_v3(g);
    const V3d r1(gen_angle(g), gen_angle(g), gen_angle(g)), r2(gen_angle(g), gen_angle(g), gen_angle(g));
    const V3d s1(gen_scale(g), gen_scale(g), gen_scale(g)), s2(gen_scale(g), gen_scale(g), gen_scale(g));
    put(p, t1);
    put(p, r1);
    put(p, s1);
    put(p, t2);
    put(p, r2);
    put(p, s2);
    put(p, pt);
    const T3d a = assemble(t1, r1, s1), c = assemble(t2, r2, s2);
    put(p, a.matrix());
    put(p, c.matrix());
    put(p, (a * c).matrix());
    put(p, V3d(a * pt));
    put(p, a.inverse().matrix());
    put(p, V3d(a.linear() * pt));
    M3d rot, scl;
    a.computeRotationScaling(&rot, &scl);
    put(p, rot);
    put(p, scl);
    V3d e = rot.eulerAngles(2, 1, 0);
    put(p, e);
    T3d x = a;
    x.pretranslate(pt);
    put(p, x.matrix());
    x = a;
    x.prerotate(Eigen::AngleAxisd(r2(0), V3d::UnitX()).toRotationMatrix());
    put(p, x.matrix());
    x = a;
    x.prescale(s2);
    put(p, x.matrix());
    x = a;
    x.scale(s2(0));
    put(p, x.matrix());
    const T3f af = a.cast<float>();
    const V3f pf = pt.cast<float>();
    put(p, af.matrix());
    put(p, V3f(af * pf));
    put(p, V3f(af.linear() * pf));
    put(p, (af * c.cast<float>()).matrix());
}

void erot_case(Payload &p, SplitMix &g)
{
    const double a1 = gen_angle(g), a2 = gen_angle(g);
    V3d ax1(gen(g, false), gen(g, false), gen(g, false)), ax2(gen(g, false), gen(g, false), gen(g, false));
    if (ax1.squaredNorm() == 0.0)
        ax1 = V3d::UnitZ();
    if (ax2.squaredNorm() == 0.0)
        ax2 = V3d::UnitY();
    ax1.normalize();
    ax2.normalize();
    const double tiny = std::ldexp(1.0 + double(g.next() >> 11) * 0x1.0p-53, -60 - int(g.below(40)));
    M3d rnd;
    for (int j = 0; j < 3; ++j)
        for (int i = 0; i < 3; ++i)
            rnd(i, j) = gen(g, false);
    p.f64(a1).f64(a2);
    put(p, ax1);
    put(p, ax2);
    p.f64(tiny);
    put(p, rnd);
    const Eigen::AngleAxisd aa1(a1, ax1), aa2(a2, ax2);
    const M3d r1 = aa1.toRotationMatrix();
    put(p, r1);
    const Eigen::Quaterniond q1(aa1), q2(aa2);
    put(p, q1.coeffs());
    put(p, (q1 * q2).coeffs());
    put(p, (aa1 * aa2).coeffs());
    const Eigen::AngleAxisf af1(float(a1), ax1.cast<float>()), af2(float(a2), ax2.cast<float>());
    put(p, (Eigen::Quaternionf(af1) * Eigen::Quaternionf(af2)).coeffs());
    put(p, M3d(q1.toRotationMatrix()));
    put(p, Eigen::Quaterniond(r1).coeffs());
    put(p, Eigen::Quaterniond(rnd).coeffs());
    const Eigen::AngleAxisd back(q1);
    p.f64(back.angle());
    put(p, back.axis());
    const Eigen::AngleAxisd fromm(r1);
    p.f64(fromm.angle());
    put(p, fromm.axis());
    const Eigen::AngleAxisd near(Eigen::Quaterniond(1.0, tiny, -tiny * 0.5, tiny * 0.25));
    p.f64(near.angle());
    put(p, near.axis());
    put(p, Eigen::Quaterniond(rnd(0, 0), rnd(1, 0), rnd(2, 0), rnd(0, 1)).normalized().coeffs());
    put(p, V3d(r1.eulerAngles(2, 1, 0)));
    put(p, V3d(r1.eulerAngles(0, 1, 2)));
    put(p, V3d(r1.eulerAngles(2, 0, 2)));
    put(p, V3d(rnd.eulerAngles(1, 0, 1)));
}

void esvd_case(Payload &p, SplitMix &g)
{
    M3d m;
    const uint64_t shape = g.below(4);
    for (int j = 0; j < 3; ++j)
        for (int i = 0; i < 3; ++i)
            m(i, j) = (shape == 0 && i != j) ? 0.0 : gen(g, false);
    if (shape == 1) // a rotation times a scaling, as Transformation keeps them
        m = assemble(V3d::Zero(), V3d(gen_angle(g), gen_angle(g), gen_angle(g)), V3d(gen_scale(g), gen_scale(g), gen_scale(g))).linear();
    put(p, m);
    const Eigen::JacobiSVD<M3d> svd(m, Eigen::ComputeFullU | Eigen::ComputeFullV);
    put(p, M3d(svd.matrixU()));
    put(p, M3d(svd.matrixV()));
    put(p, V3d(svd.singularValues()));
    p.u64(uint64_t(svd.nonzeroSingularValues()));
}

template<class F> void record_each(Sets &sets, const char (&kind)[5], size_t full, size_t committed, F f)
{
    for (size_t i = 0; i < full; ++i) {
        Payload p;
        f(p);
        sets.full.add(kind, p);
        if (i < committed)
            sets.committed.add(kind, p);
    }
}

} // namespace

void record_eigen(Sets &sets)
{
    SplitMix g(0xE16E2);
    record_evec<double, 2>(sets, g, 0, 512, 8);
    record_evec<double, 3>(sets, g, 0, 512, 8);
    record_evec<double, 4>(sets, g, 0, 512, 8);
    record_evec<float, 2>(sets, g, 1, 512, 8);
    record_evec<float, 3>(sets, g, 1, 512, 8);
    record_evec<float, 4>(sets, g, 1, 512, 8);
    record_evec<int64_t, 2>(sets, g, 2, 256, 4);
    record_evec<int64_t, 3>(sets, g, 2, 256, 4);
    record_evec<int32_t, 2>(sets, g, 3, 256, 4);
    record_evec<int32_t, 3>(sets, g, 3, 256, 4);
    record_emat<double, 2>(sets, g, 0, 256, 4);
    record_emat<double, 3>(sets, g, 0, 512, 8);
    record_emat<double, 4>(sets, g, 0, 256, 4);
    record_emat<float, 3>(sets, g, 1, 512, 8);
    record_emat<float, 4>(sets, g, 1, 256, 4);
    record_each(sets, "ETRF", 512, 6, [&](Payload &p) { etrf_case(p, g); });
    record_each(sets, "EROT", 512, 8, [&](Payload &p) { erot_case(p, g); });
    record_each(sets, "ESVD", 1024, 12, [&](Payload &p) { esvd_case(p, g); });
}

} // namespace probe
