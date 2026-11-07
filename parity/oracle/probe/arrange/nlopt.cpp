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
// arrange.sop1's optimiser records (parity/SOP1.md, "arrange.sop1"), which orca-arrange's
// tests/probes.rs replays: NLOP (NLopt 2.5.0's LN_NELDERMEAD and LN_SBPLX through nlopt::opt, every
// evaluation, the result code or the exception, the final point and value) and NOPT (libnest2d's
// TOptimizer<L_SUBPLEX>::optimize_min, with an objective that may throw). The objectives are
// arithmetic only, written the same way in the replay. Recorded by the o2-arrange task
// (2026-11-07). No time limit is ever set, so NLopt's clock never reaches a result.
#include "../probe.hpp"

#include <cmath>
#include <cstdint>
#include <exception>
#include <limits>
#include <new>
#include <stdexcept>
#include <string>
#include <tuple>
#include <vector>

#include <libnest2d/backends/libslic3r/geometries.hpp>
#include <libnest2d/optimizers/nlopt/subplex.hpp>
#include <nlopt.hpp>

namespace probe {

namespace {

// The objectives, by id, over x[0..n) with parameters c[0..4):
//   0  sum of (x[i] - c[i])^2
//   1  Rosenbrock's sum of 100 (x[i+1] - x[i]^2)^2 + (1 - x[i])^2, plus c[0]
//   2  sum of |x[i] - c[i]| * (i + 1)
//   3  NaN where x[0] > c[1], else objective 0
//   4  HUGE_VAL where x[0] < c[1], else objective 0
//   5  1 where x[0] > c[0], else 0, plus objective 0 times 1e-3
double objective(int id, unsigned n, const double *x, const double *c)
{
    auto sphere = [&]() {
        double s = 0.;
        for (unsigned i = 0; i < n; ++i) {
            const double d = x[i] - c[i];
            s += d * d;
        }
        return s;
    };
    switch (id) {
    case 0: return sphere();
    case 1: {
        double s = 0.;
        for (unsigned i = 0; i + 1 < n; ++i) {
            const double a = x[i + 1] - x[i] * x[i];
            const double b = 1. - x[i];
            s += 100. * a * a + b * b;
        }
        return s + c[0];
    }
    case 2: {
        double s = 0.;
        for (unsigned i = 0; i < n; ++i)
            s += std::fabs(x[i] - c[i]) * double(i + 1);
        return s;
    }
    case 3: return x[0] > c[1] ? std::numeric_limits<double>::quiet_NaN() : sphere();
    case 4: return x[0] < c[1] ? HUGE_VAL : sphere();
    default: return (x[0] > c[0] ? 1. : 0.) + sphere() * 1e-3;
    }
}

// An exception's kind as the records hold it: 1 std::runtime_error, 2 std::invalid_argument, 3
// std::bad_alloc, 4 nlopt::roundoff_limited, 5 nlopt::forced_stop, 9 any other std::exception.
template<class F> bool run_catching(F f, uint8_t &kind, std::string &what)
{
    try {
        f();
        return true;
    } catch (const nlopt::roundoff_limited &e) {
        kind = 4;
        what = e.what();
    } catch (const nlopt::forced_stop &e) {
        kind = 5;
        what = e.what();
    } catch (const std::invalid_argument &e) {
        kind = 2;
        what = e.what();
    } catch (const std::bad_alloc &e) {
        kind = 3;
        what = e.what();
    } catch (const std::runtime_error &e) {
        kind = 1;
        what = e.what();
    } catch (const std::exception &e) {
        kind = 9;
        what = e.what();
    }
    return false;
}

// One NLOP case. `set` says which setters run: 1 stopval, 2 ftol_rel, 4 ftol_abs, 8 xtol_rel,
// 16 xtol_abs (one value for all), 32 maxeval, 64 initial step.
struct Nlop
{
    uint8_t             alg = 0; // 0 LN_NELDERMEAD, 1 LN_SBPLX
    unsigned            n   = 1;
    std::vector<double> x0, lb, ub, dx;
    bool                maximize = false;
    uint8_t             set      = 0;
    double              stopval = 0., ftol_rel = 0., ftol_abs = 0., xtol_rel = 0., xtol_abs = 0.;
    int32_t             maxeval   = 0;
    uint8_t             obj       = 0;
    double              c[4]      = {0., 0., 0., 0.};
    int32_t             force_at  = -1; // the evaluation (from 0) whose objective calls force_stop
};

struct NlopRun
{
    const Nlop                       *k   = nullptr;
    nlopt::opt                       *opt = nullptr;
    std::vector<std::vector<double>> xs;
    std::vector<double>              fs;
};

double nlop_objective(unsigned n, const double *x, double * /*grad*/, void *data)
{
    auto        *r = static_cast<NlopRun *>(data);
    const double f = objective(r->k->obj, n, x, r->k->c);
    r->xs.emplace_back(x, x + n);
    r->fs.push_back(f);
    if (int32_t(r->fs.size()) - 1 == r->k->force_at)
        r->opt->force_stop();
    return f;
}

// The NLOP record: the case, then whether the initial step threw (and its what()), then every
// evaluation (x, f), the outcome (0 and the result code, or 1, the kind and what()), the final x
// and the value.
void nlop(Payload &p, const Nlop &k)
{
    p.u8(k.alg).u32(k.n);
    for (double v : k.x0)
        p.f64(v);
    for (double v : k.lb)
        p.f64(v);
    for (double v : k.ub)
        p.f64(v);
    p.boolean(k.maximize).u8(k.set);
    p.f64(k.stopval).f64(k.ftol_rel).f64(k.ftol_abs).f64(k.xtol_rel).f64(k.xtol_abs).i32(k.maxeval);
    for (double v : k.dx)
        p.f64(v);
    p.u8(k.obj);
    for (double v : k.c)
        p.f64(v);
    p.i32(k.force_at);

    nlopt::opt opt(k.alg == 0 ? nlopt::LN_NELDERMEAD : nlopt::LN_SBPLX, k.n);
    opt.set_lower_bounds(k.lb);
    opt.set_upper_bounds(k.ub);
    if (k.set & 1)
        opt.set_stopval(k.stopval);
    if (k.set & 2)
        opt.set_ftol_rel(k.ftol_rel);
    if (k.set & 4)
        opt.set_ftol_abs(k.ftol_abs);
    if (k.set & 8)
        opt.set_xtol_rel(k.xtol_rel);
    if (k.set & 16)
        opt.set_xtol_abs(k.xtol_abs);
    if (k.set & 32)
        opt.set_maxeval(k.maxeval);
    uint8_t     kind = 0;
    std::string what;
    if (k.set & 64) {
        const bool ok = run_catching([&] { opt.set_initial_step(k.dx); }, kind, what);
        p.boolean(!ok);
        if (!ok) {
            p.u8(kind).str(what);
            return;
        }
    } else
        p.boolean(false);

    NlopRun run;
    run.k   = &k;
    run.opt = &opt;
    if (k.maximize)
        opt.set_max_objective(nlop_objective, &run);
    else
        opt.set_min_objective(nlop_objective, &run);
    std::vector<double> x = k.x0;
    double              opt_f = 0.;
    nlopt::result       res   = nlopt::FAILURE;
    const bool          ok    = run_catching([&] { res = opt.optimize(x, opt_f); }, kind, what);

    p.u32(uint32_t(run.fs.size()));
    for (size_t i = 0; i < run.fs.size(); ++i) {
        for (double v : run.xs[i])
            p.f64(v);
        p.f64(run.fs[i]);
    }
    p.boolean(!ok);
    if (ok)
        p.i32(int32_t(res));
    else
        p.u8(kind).str(what);
    for (double v : x)
        p.f64(v);
    p.f64(opt_f);
}

// One NOPT case: libnest2d's subplex optimizer over 1, 2 or 3 variables. The objective throws
// `throw_kind` (the kinds of run_catching) at evaluation `throw_at`, after recording its input.
struct Nopt
{
    unsigned            n = 1;
    std::vector<double> x0, lb, ub;
    double              abs_diff = 0., rel_diff = 0., stop_score = 0.; // NaN: not set
    uint32_t            max_iterations = 0;
    uint8_t             obj            = 0;
    double              c[4]           = {0., 0., 0., 0.};
    int32_t             throw_at       = -1;
    uint8_t             throw_kind     = 0;
};

struct NoptRun
{
    const Nopt                       *k = nullptr;
    std::vector<std::vector<double>> xs;
    std::vector<uint8_t>             threw;
    std::vector<double>              fs;

    double eval(std::vector<double> x)
    {
        xs.push_back(x);
        if (int32_t(fs.size()) == k->throw_at) {
            threw.push_back(1);
            fs.push_back(0.);
            switch (k->throw_kind) {
            case 2: throw std::invalid_argument("probe invalid argument");
            case 3: throw std::bad_alloc();
            case 4: throw nlopt::roundoff_limited();
            case 5: throw nlopt::forced_stop();
            default: throw std::runtime_error("probe failure");
            }
        }
        const double f = objective(k->obj, unsigned(x.size()), x.data(), k->c);
        threw.push_back(0);
        fs.push_back(f);
        return f;
    }
};

void nopt(Payload &p, const Nopt &k)
{
    p.u32(k.n);
    for (double v : k.x0)
        p.f64(v);
    for (double v : k.lb)
        p.f64(v);
    for (double v : k.ub)
        p.f64(v);
    p.f64(k.abs_diff).f64(k.rel_diff).f64(k.stop_score).u32(k.max_iterations);
    p.u8(k.obj);
    for (double v : k.c)
        p.f64(v);
    p.i32(k.throw_at).u8(k.throw_kind);

    namespace opt = libnest2d::opt;
    opt::StopCriteria sc;
    sc.absolute_score_difference = k.abs_diff;
    sc.relative_score_difference = k.rel_diff;
    sc.stop_score                = k.stop_score;
    sc.max_iterations            = k.max_iterations;
    opt::TOptimizer<opt::Method::L_SUBPLEX> solver(sc);

    NoptRun run;
    run.k = &k;
    int32_t             code  = 0;
    std::vector<double> optimum;
    double              score = 0.;
    uint8_t             kind  = 0;
    std::string         what;
    const bool          ok = run_catching(
        [&] {
            if (k.n == 1) {
                auto r  = solver.optimize_min([&run](double a) { return run.eval({a}); }, opt::initvals(k.x0[0]), opt::bound(k.lb[0], k.ub[0]));
                code    = int32_t(r.resultcode);
                optimum = {std::get<0>(r.optimum)};
                score   = r.score;
            } else if (k.n == 2) {
                auto r  = solver.optimize_min([&run](double a, double b) { return run.eval({a, b}); }, opt::initvals(k.x0[0], k.x0[1]),
                                             opt::bound(k.lb[0], k.ub[0]), opt::bound(k.lb[1], k.ub[1]));
                code    = int32_t(r.resultcode);
                optimum = {std::get<0>(r.optimum), std::get<1>(r.optimum)};
                score   = r.score;
            } else {
                auto r  = solver.optimize_min([&run](double a, double b, double c) { return run.eval({a, b, c}); },
                                             opt::initvals(k.x0[0], k.x0[1], k.x0[2]), opt::bound(k.lb[0], k.ub[0]), opt::bound(k.lb[1], k.ub[1]),
                                             opt::bound(k.lb[2], k.ub[2]));
                code    = int32_t(r.resultcode);
                optimum = {std::get<0>(r.optimum), std::get<1>(r.optimum), std::get<2>(r.optimum)};
                score   = r.score;
            }
        },
        kind, what);

    p.u32(uint32_t(run.xs.size()));
    for (size_t i = 0; i < run.xs.size(); ++i) {
        for (double v : run.xs[i])
            p.f64(v);
        p.u8(run.threw[i]).f64(run.fs[i]);
    }
    p.boolean(!ok);
    if (ok) {
        p.i32(code);
        for (double v : optimum)
            p.f64(v);
        p.f64(score);
    } else
        p.u8(kind).str(what);
}

double unit(SplitMix &g) { return double(g.next() >> 11) * (1.0 / 9007199254740992.0); }
double span(SplitMix &g, double lo, double hi) { return lo + (hi - lo) * unit(g); }

// A random NLOP case. Every case stops: an evaluation limit is always set.
Nlop random_nlop(SplitMix &g)
{
    Nlop k;
    k.alg = uint8_t(g.below(2));
    k.n   = unsigned(1 + g.below(4));
    const double scale = g.below(4) == 0 ? 1e6 : (g.below(3) == 0 ? 1e-3 : 1.);
    for (unsigned i = 0; i < k.n; ++i) {
        double lo = -HUGE_VAL, hi = HUGE_VAL;
        switch (g.below(6)) {
        case 0: break;
        case 1: lo = span(g, -10., 0.) * scale; break;
        case 2: hi = span(g, 0., 10.) * scale; break;
        case 3: lo = hi = span(g, -5., 5.) * scale; break;
        default:
            lo = span(g, -10., 0.) * scale;
            hi = span(g, 0., 10.) * scale;
        }
        double x = span(g, -3., 3.) * scale;
        if (g.below(10) == 0)
            x = span(g, -30., 30.) * scale; // maybe outside its bounds
        else {
            if (x < lo)
                x = lo;
            if (x > hi)
                x = hi;
        }
        if (g.below(12) == 0)
            x = lo == -HUGE_VAL ? x : lo;
        k.x0.push_back(x);
        k.lb.push_back(lo);
        k.ub.push_back(hi);
    }
    k.maximize = g.below(6) == 0;
    k.set      = 32;
    if (g.below(3) == 0) {
        k.set |= 1;
        k.stopval = span(g, -1., 2.) * (k.maximize ? -1. : 1.);
    }
    if (g.below(2) == 0) {
        k.set |= 2;
        k.ftol_rel = g.below(3) == 0 ? 1e-20 : span(g, 0., 1e-3);
    }
    if (g.below(3) == 0) {
        k.set |= 4;
        k.ftol_abs = span(g, 0., 1e-4);
    }
    if (g.below(3) == 0) {
        k.set |= 8;
        k.xtol_rel = span(g, 0., 1e-3);
    }
    if (g.below(4) == 0) {
        k.set |= 16;
        k.xtol_abs = span(g, 0., 1e-5) * scale;
    }
    k.maxeval = int32_t(1 + g.below(g.below(3) == 0 ? 600 : 120));
    if (g.below(3) == 0) {
        k.set |= 64;
        for (unsigned i = 0; i < k.n; ++i) {
            double d = span(g, -2., 2.) * scale;
            if (g.below(15) == 0)
                d = 0.;
            if (g.below(15) == 0)
                d = 1e-300;
            k.dx.push_back(d);
        }
    }
    k.obj = uint8_t(g.below(6));
    for (double &c : k.c)
        c = span(g, -2., 2.) * scale;
    if (g.below(6) == 0)
        k.force_at = int32_t(g.below(40));
    return k;
}

Nopt random_nopt(SplitMix &g)
{
    Nopt         k;
    const double nan = std::nan("");
    k.n              = unsigned(1 + g.below(3));
    for (unsigned i = 0; i < k.n; ++i) {
        double lo = 0., hi = 1.;
        switch (g.below(4)) {
        case 0: break;
        case 1:
            lo = -1.;
            hi = 1.;
            break;
        case 2:
            lo = span(g, -100., 0.);
            hi = span(g, 0., 100.);
            break;
        default:
            lo = -HUGE_VAL;
            hi = HUGE_VAL;
        }
        double x = lo == -HUGE_VAL ? span(g, -5., 5.) : span(g, lo, hi);
        if (g.below(25) == 0)
            x = hi == HUGE_VAL ? 200. : hi + 1.; // outside: NLopt's invalid argument
        k.x0.push_back(x);
        k.lb.push_back(lo);
        k.ub.push_back(hi);
    }
    k.abs_diff       = g.below(4) == 0 ? span(g, 0., 1e-3) : nan;
    k.rel_diff       = g.below(2) == 0 ? (g.below(2) == 0 ? 1e-20 : span(g, 0., 1e-2)) : nan;
    k.stop_score     = g.below(5) == 0 ? span(g, -1., 1.) : nan;
    k.max_iterations = uint32_t(1 + g.below(g.below(3) == 0 ? 1000 : 60));
    k.obj            = uint8_t(g.below(6));
    for (double &c : k.c)
        c = span(g, -1., 1.);
    if (g.below(4) == 0) {
        k.throw_at   = int32_t(g.below(30));
        k.throw_kind = uint8_t(1 + g.below(5));
    }
    return k;
}

} // namespace

void record_nlopt(Sets &sets)
{
    // NLOP: a few chosen runs, then generated ones; the committed set keeps the smaller records.
    std::vector<Nlop> fixed;
    {
        Nlop k; // the placer's own use: subplex in [0, 1] from a corner, 1000 evaluations
        k.alg = 1;
        k.n   = 1;
        k.x0  = {0.25};
        k.lb  = {0.};
        k.ub  = {1.};
        k.set = 2 | 32;
        k.ftol_rel = 1e-20;
        k.maxeval  = 1000;
        k.obj      = 0;
        k.c[0]     = 0.7;
        fixed.push_back(k);
        k.alg = 0;
        fixed.push_back(k);
        k.alg      = 1;
        k.n        = 2; // minimizeCircle's: two variables in [-1, 1], 30 evaluations
        k.x0       = {0., 0.};
        k.lb       = {-1., -1.};
        k.ub       = {1., 1.};
        k.ftol_rel = 1e-3;
        k.maxeval  = 30;
        k.obj      = 2;
        k.c[0]     = 0.3;
        k.c[1]     = -0.6;
        fixed.push_back(k);
        k.x0  = {0.5, 3.}; // outside its bounds
        fixed.push_back(k);
        k.x0  = {0.5, 0.5}; // a fixed dimension, eliminated
        k.lb  = {-1., 0.5};
        k.ub  = {1., 0.5};
        k.obj = 0;
        fixed.push_back(k);
        k.force_at = 3; // forced stop while a dimension is eliminated: the copy never sees it
        fixed.push_back(k);
        k.lb       = {-1., -1.};
        k.ub       = {1., 1.};
        fixed.push_back(k); // forced stop
        k.force_at = -1;
        k.set |= 64; // a step that makes the simplex too small: NLopt's own failure message
        k.dx       = {1e-300, 0.5};
        k.alg      = 0;
        fixed.push_back(k);
        k.dx = {0., 0.5}; // a zero step: set_initial_step throws
        fixed.push_back(k);
        k.set      = 32 | 1;
        k.dx       = {};
        k.stopval  = 0.5;
        k.maximize = true;
        k.obj      = 3;
        fixed.push_back(k);
    }
    for (const Nlop &k : fixed) {
        Payload p;
        nlop(p, k);
        sets.full.add("NLOP", p);
        sets.committed.add("NLOP", p);
    }
    SplitMix g(0x4E4C4F50u);
    size_t   taken = 0;
    for (int i = 0; i < 1500; ++i) {
        const Nlop k = random_nlop(g);
        Payload    p;
        nlop(p, k);
        sets.full.add("NLOP", p);
        if (taken < 60 && p.bytes.size() <= 2048) {
            sets.committed.add("NLOP", p);
            ++taken;
        }
    }

    // NOPT
    std::vector<Nopt> nfixed;
    {
        const double nan = std::nan("");
        Nopt         k; // the placer's: one variable in [0, 1], relative difference 1e-20
        k.n              = 1;
        k.x0             = {0.4};
        k.lb             = {0.};
        k.ub             = {1.};
        k.abs_diff       = nan;
        k.rel_diff       = 1e-20;
        k.stop_score     = nan;
        k.max_iterations = 650;
        k.obj            = 0;
        k.c[0]           = 0.123;
        nfixed.push_back(k);
        for (uint8_t kind = 1; kind <= 5; ++kind) {
            k.throw_at   = 4;
            k.throw_kind = kind;
            nfixed.push_back(k);
        }
        k.throw_at = 0; // at the first evaluation
        k.throw_kind = 5;
        nfixed.push_back(k);
        k.throw_at = -1;
        k.x0       = {1.5}; // outside its bounds: std::invalid_argument leaves the call
        nfixed.push_back(k);
    }
    for (const Nopt &k : nfixed) {
        Payload p;
        nopt(p, k);
        sets.full.add("NOPT", p);
        sets.committed.add("NOPT", p);
    }
    SplitMix h(0x4E4F5054u);
    taken = 0;
    for (int i = 0; i < 1200; ++i) {
        const Nopt k = random_nopt(h);
        Payload    p;
        nopt(p, k);
        sets.full.add("NOPT", p);
        if (taken < 50 && p.bytes.size() <= 2048) {
            sets.committed.add("NOPT", p);
            ++taken;
        }
    }
}

} // namespace probe
