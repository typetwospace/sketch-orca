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
// ANBS (parity/sop1/arachne.md): the beading strategies and the factory's stack, piece arachne-1.
// Each record is one strategy, built by BeadingStrategyFactory::makeStrategy or as a chain of the
// five strategies one on another: its name, thresholds and angle, for each lower bead count its
// optimal and transition thicknesses, transition length and anchor and its nonlinear thicknesses,
// and for thicknesses across its widths the optimal bead count and compute() for that count and
// its neighbours. Recorded by orca-o3-probe-arachne, 2026-11-13.
#include "common.hpp"

#include <algorithm>
#include <set>

#include "libslic3r/Geometry.hpp"

namespace probe::arachne {

namespace {

struct Named
{
    std::string  name;
    StrategySpec spec;
    bool         committable;
};

Layer D(coord_t w, coord_t tl, double angle, double split, double add, int radius) { return {0, w, tl, angle, split, add, radius}; }
Layer R(coord_t outer, double ratio) { return {1, outer, 0, ratio}; }
Layer W(coord_t min_in, coord_t min_out) { return {2, min_in, min_out}; }
Layer O(coord_t offset) { return {3, offset}; }
Layer L(coord_t max) { return {4, max}; }

StrategySpec chain(std::vector<Layer> layers)
{
    StrategySpec s;
    s.factory = false;
    s.chain   = std::move(layers);
    return s;
}

StrategySpec factory(const FactoryArgs &a)
{
    StrategySpec s;
    s.args = a;
    return s;
}

std::vector<Named> strategies()
{
    std::vector<Named> out;
    const double       deg10 = double(float(Slic3r::Geometry::deg2rad(10.f)));
    const double       deg30 = double(float(Slic3r::Geometry::deg2rad(30.f)));
    // Factory stacks as WallToolPaths builds them (the domain-A settings, nozzles 0.4, 0.6, 0.25).
    for (size_t insets : {1, 2, 3, 4})
        out.push_back({"factory 0.4 insets " + std::to_string(insets), factory(wall_args(0.407, 0.407, 0.4, insets, 0)), insets <= 2});
    out.push_back({"factory 0.4 insets 2 precise outer wall", factory(wall_args(0.407, 0.407, 0.4, 2, -21500)), true});
    out.push_back({"factory 0.4 insets 3 outer 0.42 inner 0.45", factory(wall_args(0.42, 0.45, 0.4, 3, 0)), false});
    {
        FactoryArgs a      = wall_args(0.407, 0.407, 0.4, 2, 0);
        a.print_thin_walls = false;
        out.push_back({"factory 0.4 insets 2 no thin walls", factory(a), true});
    }
    {
        FactoryArgs a                          = wall_args(0.407, 0.407, 0.4, 3, 0);
        a.inward_distributed_center_wall_count = 2;
        a.transitioning_angle                  = float(Slic3r::Geometry::deg2rad(30.f));
        out.push_back({"factory 0.4 insets 3 distribution 2 angle 30", factory(a), false});
    }
    {
        FactoryArgs a                          = wall_args(0.407, 0.407, 0.4, 4, 0);
        a.inward_distributed_center_wall_count = 3;
        a.minimum_variable_line_ratio          = 0.3;
        out.push_back({"factory 0.4 insets 4 distribution 3 ratio 0.3", factory(a), false});
    }
    out.push_back({"factory 0.6 insets 2", factory(wall_args(0.62, 0.62, 0.6, 2, 0)), false});
    out.push_back({"factory 0.25 insets 3", factory(wall_args(0.26, 0.26, 0.25, 3, 0)), false});
    out.push_back({"factory 0.4 insets 6", factory(wall_args(0.407, 0.407, 0.4, 6, 0)), false});
    out.push_back({"factory 0.4 insets 2 outer offset 30000", factory(wall_args(0.407, 0.407, 0.4, 2, 30000)), false});
    {
        // The factory's defaults (BeadingStrategyFactory.hpp), with a bead count limit.
        FactoryArgs a;
        a.outer = a.inner = scaled<coord_t>(0.0005);
        a.transition_length = scaled<coord_t>(0.0004);
        a.transitioning_angle = float(M_PI / 4.0);
        a.max_bead_count      = 4;
        out.push_back({"factory defaults max 4", factory(a), false});
    }
    // Each strategy on its own and in chains.
    const coord_t w = 400000, tl = 400000;
    out.push_back({"distributed radius 1", chain({D(w, tl, deg10, 0.5, 0.5, 1)}), true});
    out.push_back({"distributed radius 2 split 0.7 add 0.3", chain({D(w, tl, deg10, 0.7, 0.3, 2)}), true});
    out.push_back({"distributed radius 3", chain({D(w, tl, deg30, 0.51, 0.75, 3)}), false});
    out.push_back({"redistribute on distributed", chain({D(w, tl, deg10, 0.51, 0.75, 1), R(450000, 0.5)}), true});
    out.push_back({"widening on redistribute", chain({D(w, tl, deg10, 0.51, 0.75, 1), R(450000, 0.5), W(100000, 340000)}), true});
    out.push_back({"outer wall inset -21500", chain({D(w, tl, deg10, 0.51, 0.75, 1), R(420000, 0.5), O(-21500)}), false});
    out.push_back({"outer wall inset 50000", chain({D(w, tl, deg10, 0.51, 0.75, 1), R(420000, 0.5), O(50000)}), false});
    out.push_back({"limited 2 on distributed", chain({D(w, tl, deg10, 0.5, 0.5, 1), L(2)}), true});
    out.push_back({"limited 3 on distributed (odd)", chain({D(w, tl, deg10, 0.5, 0.5, 1), L(3)}), true});
    out.push_back({"limited 4 on redistribute", chain({D(w, tl, deg10, 0.51, 0.75, 1), R(450000, 0.5), L(4)}), false});
    out.push_back({"limited 6 on widening", chain({D(w, tl, deg10, 0.51, 0.75, 2), R(450000, 0.5), W(100000, 340000), L(6)}), false});
    out.push_back({"limited 4 on outer wall inset on widening", chain({D(w, tl, deg10, 0.51, 0.75, 1), W(150000, 300000), O(-30000), L(4)}), false});
    out.push_back({"widening on distributed", chain({D(w, tl, deg10, 0.5, 0.5, 1), W(100000, 340000)}), false});
    return out;
}

// The largest bead count a strategy's records go to: the limit and one more for a limited stack,
// ten for one without a limit.
coord_t top_count(const StrategySpec &s)
{
    if (s.factory)
        return std::max<coord_t>(s.args.max_bead_count, 2) + 1;
    for (const Layer &l : s.chain)
        if (l.kind == 4)
            return std::max<coord_t>(l.a, 2) + 1;
    return 10;
}

coord_t base_width(const StrategySpec &s)
{
    if (s.factory)
        return std::max(s.args.outer, s.args.inner);
    return s.chain.front().a;
}

} // namespace

void record_beading(Out &out)
{
    for (const Named &n : strategies()) {
        Case    c;
        Payload p;
        p.str(n.name);
        put(p, n.spec);
        log_take();
        BeadingStrategyPtr strat = make_strategy(n.spec);
        put(p, log_take());
        p.str(strat->toString()).f64(strat->getSplitMiddleThreshold()).f64(strat->getTransitioningAngle());
        const coord_t top = top_count(n.spec);
        p.u64(uint64_t(top + 1));
        std::set<coord_t> special;
        for (coord_t k = 0; k <= top; ++k) {
            const coord_t opt   = strat->getOptimalThickness(k);
            const coord_t trans = strat->getTransitionThickness(k);
            p.i64(opt).i64(trans).i64(strat->getTransitioningLength(k)).f32(strat->getTransitionAnchorPos(k));
            put_i64s(p, strat->getNonlinearThicknesses(k));
            if (k < top) {
                for (coord_t d : {-1, 0, 1})
                    if (trans + d >= 0)
                        special.insert(trans + d);
                special.insert(opt);
            }
        }
        // Thicknesses across the widths: a grid of a fifth of the bead width, then the transition
        // and optimal thicknesses themselves, where compute() also takes one bead fewer.
        const coord_t w = base_width(n.spec);
        struct Sample
        {
            coord_t t;
            bool    fewer;
        };
        std::vector<Sample> samples;
        for (coord_t t = 0; t <= (top + 1) * w; t += std::max<coord_t>(w / 5, 1))
            samples.push_back({t, false});
        for (coord_t t : special)
            samples.push_back({t, true});
        p.u64(samples.size());
        for (const Sample &s : samples) {
            const coord_t count = strat->getOptimalBeadCount(s.t);
            p.i64(s.t).i64(count);
            std::vector<coord_t> counts;
            if (s.fewer && count > 0)
                counts.push_back(count - 1);
            counts.push_back(count);
            counts.push_back(count + 1);
            p.u64(counts.size());
            for (coord_t k : counts) {
                p.i64(k);
                put(p, strat->compute(s.t, k));
            }
        }
        put(p, log_take());
        c.add("ANBS", p);
        out.put("beading", c, n.committable);
    }
}

} // namespace probe::arachne
