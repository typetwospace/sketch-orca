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
// surfaces.sop1's MutablePolygon records (parity/sop1/surfaces.md, "SFMP", "SFRD" and "SFSO",
// added 2026-11-13 by orca-o3-probe-surfaces), which orca-surface replays for the piece
// surfaces-1:
//
//   SFMP  a script of MutablePolygon operations (its iterators, insert and remove, its range of
//         unprocessed points, assign, clear, polygon() and ==), with the state after each step;
//   SFRD  the three remove_duplicates of a MutablePolygon and the ExPolygons one;
//   SFSO  smooth_outward of a Polygon, of Polygons and of ExPolygons (clip_narrow_corner inside).
//
// Every script keeps to what the class allows: no step reads an iterator that is not valid, and
// no point a range ends at is removed but through the range's own remove_front and remove_back,
// as smooth_outward and clip_narrow_corner use it. Domain A never reaches this code (smooth_outward
// is called by clip_fill_surfaces, which returns at once as PrintObject::infill_only_where_needed
// is false, and by the supports; the ExPolygons remove_duplicates by the multi-material
// segmentation), but its port carries it all.
#include "shared.hpp"

#include <optional>

#include "libslic3r/ClipperUtils.hpp"
#include "libslic3r/MutablePolygon.hpp"

namespace probe {

void record_mutable(Sets &sets);

namespace {

using namespace probe::surfaces;
using Slic3r::MutablePolygon;

// ---------------------------------------------------------------- SFMP

enum Op : uint8_t {
    opNext,
    opPrev,
    opInsert,
    opRemove,
    opBegin,
    opEnd,
    opPolygon,
    opClear,
    opAssign,
    opCompare,
    opRange,
    opProcessNext,
    opAdvanceFront,
    opRetractBack,
    opRemoveFront,
    opRemoveBack,
    opSet,
    opCount
};

struct Script
{
    MutablePolygon                       mp;
    MutablePolygon::iterator             it;
    std::optional<MutablePolygon::range> range;
    Script() : it(mp.begin()) {}
};

// The state after a step: u64 size, u8 empty, u8 valid, u8 the cursor is valid; if it is, its
// point, u8 it is begin(), u8 it is end(); u8 a range is held; if one is, u8 it is empty, and when
// it is not, its begin's and end's points, u8 begin is the cursor, u8 end is the cursor.
void put_state(Payload &p, Script &s)
{
    p.u64(s.mp.size()).boolean(s.mp.empty()).boolean(s.mp.valid()).boolean(s.it.valid());
    if (s.it.valid()) {
        put(p, *s.it);
        p.boolean(s.it == s.mp.begin()).boolean(s.it == s.mp.end());
    }
    p.boolean(s.range.has_value());
    if (s.range) {
        p.boolean(s.range->empty());
        if (!s.range->empty()) {
            put(p, *s.range->begin());
            put(p, *s.range->end());
            const bool v = s.it.valid();
            p.boolean(v && s.range->begin() == s.it).boolean(v && s.range->end() == s.it);
        }
    }
}

Point near_point(SplitMix &g, const MutablePolygon &mp, int64_t r)
{
    // Often a point of the polygon itself (duplicates), else one near the first point.
    if (!mp.empty() && g.below(4) == 0)
        return *mp.cbegin();
    return Point(int64_t(g.below(uint64_t(2 * r + 1))) - r, int64_t(g.below(uint64_t(2 * r + 1))) - r);
}

Points script_points(SplitMix &g, int64_t r)
{
    const int n     = int(g.below(9));
    Points    pts   = scatter(g, n, 0, 0, r);
    if (n > 1 && g.below(3) == 0)
        pts[1] = pts[0]; // a duplicate
    return pts;
}

// Whether the step `op` may run now: it reads only valid iterators and removes no range end but
// through the range.
bool allowed(Script &s, uint8_t op)
{
    const bool valid = s.it.valid();
    const bool rng   = s.range.has_value();
    const bool rne   = rng && !s.range->empty();
    const bool at_b  = rne && valid && s.range->begin() == s.it;
    const bool at_e  = rne && valid && s.range->end() == s.it;
    switch (op) {
    case opNext:
    case opPrev:
    case opInsert:
    case opSet: return valid;
    case opRemove: return valid && !at_b && !at_e;
    case opRange: return !s.mp.empty();
    case opProcessNext:
    case opAdvanceFront:
    case opRetractBack: return rne;
    case opRemoveFront: return rng && valid && !(at_e && !at_b);
    case opRemoveBack: return rng && valid && !(at_b && !at_e);
    default: return true;
    }
}

Payload sfmp_case(SplitMix &g, int steps)
{
    Script        s;
    Payload       p;
    const int64_t r   = int64_t(1) << (4 + g.below(24));
    const uint8_t how = uint8_t(g.below(3));
    const Points  pts = script_points(g, r);
    const size_t  reserve = size_t(g.below(3) == 0 ? g.below(20) : 0);
    p.u8(how);
    if (how == 0) {
        s.mp.assign(pts.begin(), pts.end(), reserve);
    } else if (how == 1) {
        const Points before = script_points(g, r);
        put(p, before);
        s.mp.assign(Polygon(before), 0);
        s.mp.assign(Polygon(pts), reserve);
    } else {
        const Polygon  poly(pts);
        MutablePolygon fresh(poly, reserve);
        s.mp = fresh;
    }
    s.it = s.mp.begin();
    put(p, pts);
    p.u64(reserve);
    put_state(p, s);
    p.u32(uint32_t(steps));
    for (int i = 0; i < steps; ++i) {
        uint8_t op;
        do
            op = uint8_t(g.below(opCount));
        while (!allowed(s, op));
        p.u8(op);
        switch (op) {
        case opNext: ++s.it; break;
        case opPrev: --s.it; break;
        case opInsert: {
            const Point q = near_point(g, s.mp, r);
            put(p, q);
            s.it = s.it.insert(q);
            break;
        }
        case opRemove: s.it.remove(); break;
        case opBegin: s.it = s.mp.begin(); break;
        case opEnd: s.it = s.mp.end(); break;
        case opPolygon: put(p, s.mp.polygon()); break;
        case opClear:
            s.mp.clear();
            s.it = s.mp.begin();
            s.range.reset();
            break;
        case opAssign: {
            const Points q  = script_points(g, r);
            const size_t rv = size_t(g.below(10));
            put(p, q);
            p.u64(rv);
            s.mp.assign(q.begin(), q.end(), rv);
            s.it = s.mp.begin();
            s.range.reset();
            break;
        }
        case opCompare: {
            // Against the polygon's own points, turned by one now and then, or other points.
            Points q = s.mp.polygon().points;
            if (g.below(3) == 0 && q.size() > 1)
                std::rotate(q.begin(), q.begin() + 1, q.end());
            else if (g.below(3) == 0)
                q = script_points(g, r);
            put(p, q);
            const Polygon        qp(q);
            const MutablePolygon other(qp);
            p.boolean(s.mp == other).boolean(s.mp != other);
            break;
        }
        case opRange: s.range.emplace(s.mp); break;
        case opProcessNext: s.it = s.range->process_next(); break;
        case opAdvanceFront: s.range->advance_front(); break;
        case opRetractBack: s.range->retract_back(); break;
        case opRemoveFront: s.it = s.range->remove_front(s.it); break;
        case opRemoveBack: s.it = s.range->remove_back(s.it); break;
        case opSet: {
            const Point q = near_point(g, s.mp, r);
            put(p, q);
            *s.it = q;
            break;
        }
        }
        put_state(p, s);
    }
    return p;
}

// ---------------------------------------------------------------- SFRD and SFSO inputs

// A polygon with near duplicates, collinear runs and sharp spikes, within `r` of the origin.
Polygon noisy(SplitMix &g, int64_t r)
{
    Polygon     base = g.below(2) ? star(g, 3 + int(g.below(14)), 0., 0., double(r) * 1e-6) : Polygon(scatter(g, 3 + int(g.below(10)), 0, 0, r));
    Polygon     out;
    const int64_t jitter = int64_t(1) << g.below(14);
    for (const Point &q : base.points) {
        out.points.push_back(q);
        const uint64_t k = g.below(6);
        if (k == 0)
            out.points.push_back(q); // an exact duplicate
        else if (k == 1)
            out.points.emplace_back(q.x() + int64_t(g.below(uint64_t(2 * jitter + 1))) - jitter, q.y() + int64_t(g.below(uint64_t(2 * jitter + 1))) - jitter);
    }
    if (g.below(2))
        out = reversed(out);
    return out;
}

// A comb: a rectangle (mm) with slits cut into its top edge, some narrower than `clip`, some
// slanted, and V notches; reversed when `cw`.
Polygon comb(SplitMix &g, double clip_mm, bool cw)
{
    const double w = uniform(g, 2., 20.), h = uniform(g, 2., 10.);
    Points       pts{Point(0, 0), Point(mm(w), 0)};
    // The top edge from right to left, with teeth.
    double       x     = w;
    const int    teeth = 1 + int(g.below(6));
    for (int i = 0; i < teeth && x > 0.5; ++i) {
        const double step  = uniform(g, 0.2, w / teeth + 0.5);
        const double slit  = uniform(g, 0.1, 3.) * clip_mm;
        const double depth = uniform(g, 0.05, 0.9) * h;
        const double slant = g.below(3) == 0 ? uniform(g, -2., 2.) * slit : 0.;
        x -= step;
        if (x - slit < 0.)
            break;
        pts.emplace_back(mm(x), mm(h));
        if (g.below(3) == 0) {
            // A V notch.
            pts.emplace_back(mm(x - slit / 2 + slant), mm(h - depth));
        } else {
            pts.emplace_back(mm(x + slant), mm(h - depth));
            pts.emplace_back(mm(x - slit + slant), mm(h - depth));
        }
        pts.emplace_back(mm(x - slit), mm(h));
        x -= slit;
    }
    pts.emplace_back(0, mm(h));
    Polygon p(pts);
    return cw ? reversed(p) : p;
}

// The hand-made SFSO polygons: a square, cracks narrower and wider than the clip distance, a
// spike, tiny CW triangles (holes to be closed), a CCW triangle, obtuse corners near and far from
// the bisector, and a notch whose arms are long.
std::vector<Polygon> sfso_hand(int64_t clip)
{
    const double c = double(clip) * 1e-6;
    std::vector<Polygon> out;
    out.push_back(rect(0, 0, 10, 10));
    // A crack narrower than clip.
    out.push_back(Polygon(Points{Point(0, 0), Point(mm(10), 0), Point(mm(10), mm(10)), Point(mm(5) + clip / 4, mm(10)), Point(mm(5), mm(2)), Point(mm(5) - clip / 4, mm(10)), Point(0, mm(10))}));
    // A crack a little wider than clip, then much wider.
    out.push_back(Polygon(Points{Point(0, 0), Point(mm(10), 0), Point(mm(10), mm(10)), Point(mm(5) + clip * 6 / 10, mm(10)), Point(mm(5), mm(5)), Point(mm(5) - clip * 6 / 10, mm(10)), Point(0, mm(10))}));
    out.push_back(Polygon(Points{Point(0, 0), Point(mm(10), 0), Point(mm(10), mm(10)), Point(mm(5) + 3 * clip, mm(10)), Point(mm(5), mm(5)), Point(mm(5) - 3 * clip, mm(10)), Point(0, mm(10))}));
    // A slit with a widening end (the trapezoid trimmed), and a slit that is blocked on one side.
    out.push_back(Polygon(Points{Point(0, 0), Point(mm(10), 0), Point(mm(10), mm(10)), Point(mm(6), mm(10)), Point(mm(5) + clip / 3, mm(8)), Point(mm(5) + clip / 3, mm(3)), Point(mm(5) - clip / 3, mm(3)), Point(mm(5) - clip / 3, mm(8)), Point(mm(4), mm(10)), Point(0, mm(10))}));
    out.push_back(Polygon(Points{Point(0, 0), Point(mm(10), 0), Point(mm(10), mm(10)), Point(mm(5) + clip / 3, mm(10)), Point(mm(5) + clip / 3, mm(3)), Point(mm(5) - clip / 3, mm(3)), Point(mm(3), mm(10)), Point(0, mm(10))}));
    // Tiny CW triangles: a low one (a hole to close) and a tall one.
    out.push_back(reversed(Polygon(Points{Point(0, 0), Point(clip * 4, 0), Point(clip * 2, clip / 2)})));
    out.push_back(reversed(Polygon(Points{Point(0, 0), Point(clip * 4, 0), Point(clip * 2, clip * 3)})));
    out.push_back(Polygon(Points{Point(0, 0), Point(clip * 4, 0), Point(clip * 2, clip / 2)}));
    // Obtuse concave corners: arms of equal length, very unequal, and almost straight.
    out.push_back(Polygon(Points{Point(0, 0), Point(mm(10), 0), Point(mm(10), mm(10)), Point(mm(5), mm(7)), Point(0, mm(10))}));
    out.push_back(Polygon(Points{Point(0, 0), Point(mm(10), 0), Point(mm(10), mm(10)), Point(mm(9), mm(9)), Point(0, mm(10))}));
    out.push_back(Polygon(Points{Point(0, 0), Point(mm(10), 0), Point(mm(10), mm(10)), Point(mm(5), mm(10) - clip / 8), Point(0, mm(10))}));
    out.push_back(Polygon(Points{Point(0, 0), Point(mm(10), 0), Point(mm(10), mm(10)), Point(mm(5), mm(10) - clip), Point(0, mm(10))}));
    // A square hole (CW) and a hole with a narrow inward crack.
    out.push_back(reversed(rect(0, 0, 10, 10)));
    out.push_back(reversed(out[1]));
    // A sharp spike outwards and a narrow inward wedge with long arms.
    out.push_back(Polygon(Points{Point(0, 0), Point(mm(10), 0), Point(mm(10), mm(10)), Point(mm(5), mm(30)), Point(mm(5) - clip / 5, mm(10)), Point(0, mm(10))}));
    out.push_back(Polygon(Points{Point(0, 0), Point(mm(30), 0), Point(mm(30), mm(10)), Point(mm(1), mm(10) - clip / 5), Point(mm(30), mm(10) - clip / 2), Point(mm(30), mm(20)), Point(0, mm(20))}));
    (void)c;
    return out;
}

void record_sfmp(Sets &sets)
{
    SplitMix g(0x5F3E0001u);
    Picker   pick{50, 1200};
    for (int i = 0; i < 2000; ++i)
        add(sets, "SFMP", sfmp_case(g, 4 + int(g.below(i < 200 ? 30 : 80))), pick);
}

// SFRD: u8 variant (0 exact, 1 eps, 2 scaled eps and angle, 3 the ExPolygons overload); for 0 to 2
// the polygon in, then f64 eps (1) or i64 scaled eps and f64 max angle (2), the polygon out; for 3
// the expolygons in, i64 scaled eps, f64 max angle, the expolygons out.
void record_sfrd(Sets &sets)
{
    SplitMix g(0x5F3E0002u);
    Picker   pick{80, 1500};
    auto     one = [&](uint8_t variant, const Polygon &in, double eps, int64_t seps, double angle) {
        Payload p;
        p.u8(variant);
        put(p, in);
        MutablePolygon mp(in);
        if (variant == 0) {
            Slic3r::remove_duplicates(mp);
        } else if (variant == 1) {
            p.f64(eps);
            Slic3r::remove_duplicates(mp, eps);
        } else {
            p.i64(seps).f64(angle);
            Slic3r::remove_duplicates(mp, ::coord_t(seps), angle);
        }
        put(p, mp.polygon());
        add(sets, "SFRD", p, pick);
    };
    auto ex = [&](const ExPolygons &in, int64_t seps, double angle) {
        Payload p;
        p.u8(3);
        put(p, in);
        p.i64(seps).f64(angle);
        put(p, Slic3r::remove_duplicates(in, ::coord_t(seps), angle));
        add(sets, "SFRD", p, pick);
    };
    // Hand-made: empty, one point, two equal points, all equal, a square with duplicates at the
    // seam, a spike within eps, a collinear run.
    const std::vector<Polygon> hand = {
        Polygon(),
        Polygon(Points{Point(5, 5)}),
        Polygon(Points{Point(5, 5), Point(5, 5)}),
        Polygon(Points{Point(5, 5), Point(5, 5), Point(5, 5), Point(5, 5)}),
        Polygon(Points{Point(0, 0), Point(0, 0), Point(100, 0), Point(100, 100), Point(0, 100), Point(0, 0)}),
        Polygon(Points{Point(0, 0), Point(1000, 0), Point(1003, 2), Point(1000, 4), Point(1000, 1000), Point(0, 1000)}),
        Polygon(Points{Point(0, 0), Point(10, 0), Point(20, 0), Point(21, 0), Point(30, 0), Point(30, 30), Point(0, 30)}),
        Polygon(Points{Point(0, 0), Point(10000, 0), Point(10000, 7), Point(10003, 9), Point(10000, 10000), Point(0, 10000), Point(0, 5)}),
    };
    for (const Polygon &h : hand) {
        one(0, h, 0, 0, 0);
        one(1, h, 3., 0, 0);
        one(1, h, 10., 0, 0);
        one(2, h, 0, 10000, 3.141592653589793 / 6);
        one(2, h, 0, 5, 0.);
        one(2, h, 0, 10, 3.141592653589793 / 2);
    }
    ex(ExPolygons{}, 10000, 3.141592653589793 / 6);
    {
        ExPolygon e(rect(0, 0, 1, 1));
        e.holes.push_back(reversed(Polygon(Points{Point(mm(0.4), mm(0.4)), Point(mm(0.4) + 3, mm(0.4)), Point(mm(0.4) + 3, mm(0.4) + 3)})));
        e.holes.push_back(reversed(rect(0.6, 0.6, 0.8, 0.8)));
        ex(ExPolygons{e, ExPolygon(Polygon(Points{Point(0, 0), Point(4, 0), Point(4, 4)}))}, 10000, 3.141592653589793 / 6);
    }
    for (int i = 0; i < 1500; ++i) {
        const int64_t r = int64_t(1) << (8 + g.below(22));
        const Polygon in = noisy(g, r);
        switch (g.below(4)) {
        case 0: one(0, in, 0, 0, 0); break;
        case 1: one(1, in, std::ldexp(1., int(g.below(16))) * uniform(g, 0.5, 1.5), 0, 0); break;
        case 2: {
            // MultiMaterialSegmentation's call: scaled 0.01 mm and PI / 6; and other pairs.
            const bool mm_call = g.below(3) == 0;
            one(2, in, 0, mm_call ? 10000 : int64_t(1) << g.below(20), mm_call ? 3.141592653589793 / 6 : uniform(g, 0., 3.2));
            break;
        }
        default: {
            ExPolygons exs;
            const int  k = 1 + int(g.below(3));
            for (int j = 0; j < k; ++j) {
                ExPolygon e(noisy(g, r));
                if (g.below(2))
                    e.holes.push_back(reversed(noisy(g, r / 4)));
                exs.push_back(e);
            }
            ex(exs, g.below(3) == 0 ? 10000 : int64_t(1) << g.below(20), g.below(3) == 0 ? 3.141592653589793 / 6 : uniform(g, 0., 3.2));
        }
        }
    }
}

// SFSO: u8 variant (0 a Polygon, 1 Polygons, 2 ExPolygons), i64 clip distance, the input, the
// output of that overload.
void record_sfso(Sets &sets)
{
    SplitMix g(0x5F3E0003u);
    Picker   pick{90, 2500};
    auto     poly = [&](const Polygon &in, int64_t clip) {
        Payload p;
        p.u8(0).i64(clip);
        put(p, in);
        put(p, Slic3r::smooth_outward(in, ::coord_t(clip)));
        add(sets, "SFSO", p, pick);
    };
    auto polys = [&](const Polygons &in, int64_t clip) {
        Payload p;
        p.u8(1).i64(clip);
        put(p, in);
        put(p, Slic3r::smooth_outward(in, ::coord_t(clip)));
        add(sets, "SFSO", p, pick);
    };
    auto exs = [&](const ExPolygons &in, int64_t clip) {
        Payload p;
        p.u8(2).i64(clip);
        put(p, in);
        put(p, Slic3r::smooth_outward(in, ::coord_t(clip)));
        add(sets, "SFSO", p, pick);
    };
    // clip_fill_surfaces's distance, scaled 0.1 mm, and a larger and a smaller one.
    for (int64_t clip : {int64_t(100000), int64_t(500000), int64_t(20000)})
        for (const Polygon &h : sfso_hand(clip))
            poly(h, clip);
    polys(Polygons{}, 100000);
    {
        const std::vector<Polygon> hand = sfso_hand(100000);
        polys(Polygons(hand.begin(), hand.end()), 100000);
    }
    exs(ExPolygons{}, 100000);
    {
        ExPolygon e(rect(-5, -5, 15, 15));
        e.holes.push_back(reversed(sfso_hand(100000)[1]));
        e.holes.push_back(reversed(Polygon(Points{Point(mm(12), mm(12)), Point(mm(12) + 400000, mm(12)), Point(mm(12) + 200000, mm(12) + 50000)})));
        exs(ExPolygons{e}, 100000);
    }
    for (int i = 0; i < 1500; ++i) {
        const int64_t clip = g.below(3) == 0 ? 100000 : int64_t(uniform(g, 2e3, 2e6));
        const double  cmm  = double(clip) * 1e-6;
        switch (g.below(5)) {
        case 0:
        case 1: poly(comb(g, cmm, g.below(2) != 0), clip); break;
        case 2: poly(noisy(g, int64_t(1) << (14 + g.below(10))), clip); break;
        case 3: {
            Polygons in;
            const int k = 1 + int(g.below(3));
            for (int j = 0; j < k; ++j)
                in.push_back(g.below(2) ? comb(g, cmm, g.below(2) != 0) : noisy(g, int64_t(1) << (14 + g.below(10))));
            polys(in, clip);
            break;
        }
        default: {
            // closing(...) then smooth_outward, as clip_fill_surfaces does, on a random region.
            const ExPolygons rg = region(g, 4, 20.);
            exs(Slic3r::closing_ex(Slic3r::to_polygons(rg), float(Slic3r::scaled<float>(2.f)), Slic3r::ClipperLib::jtSquare, 0.), clip);
        }
        }
    }
}

} // namespace

void record_mutable(Sets &sets)
{
    record_sfmp(sets);
    record_sfrd(sets);
    record_sfso(sets);
}

} // namespace probe
