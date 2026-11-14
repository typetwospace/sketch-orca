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
// ANSC and ANST (pieces arachne-3 and arachne-4), ANGR (arachne-2), ANWS and ANIP (arachne-4),
// parity/sop1/arachne.md: SkeletalTrapezoidation run step by step in generateToolpaths' order
// (its protected steps reached through a derived class, never copied), with the graph after each
// step and the toolpaths at the end, checked against a direct generateToolpaths; the graph's own
// operations on copies of those graphs; WallToolPaths' protected post-processing steps on the
// toolpaths; and the two interpolate overloads. The inputs are the outlines WallToolPaths::generate
// would hand over for the probe's shapes and the test meshes' slices. Recorded by
// orca-o3-probe-arachne, 2026-11-13.
#include "common.hpp"

#include <algorithm>
#include <iostream>
#include <list>

#include "libslic3r/Arachne/SkeletalTrapezoidation.hpp"
#include "libslic3r/ClipperUtils.hpp"

namespace probe::arachne {

namespace {

using ST   = Slic3r::Arachne::SkeletalTrapezoidation;
using Mids = std::vector<std::shared_ptr<std::list<Slic3r::Arachne::SkeletalTrapezoidationEdge::TransitionMiddle>>>;
using Ends = std::vector<std::shared_ptr<std::list<Slic3r::Arachne::SkeletalTrapezoidationEdge::TransitionEnd>>>;

// SkeletalTrapezoidation's protected steps, called in generateToolpaths' order.
class Staged : public ST
{
public:
    using ST::ST;
    using ST::applyTransitions;
    using ST::filterCentral;
    using ST::filterNoncentralRegions;
    using ST::filterTransitionMids;
    using ST::generateAllTransitionEnds;
    using ST::generateExtraRibs;
    using ST::generateSegments;
    using ST::generateTransitionMids;
    using ST::interpolate;
    using ST::p_generated_toolpaths;
    using ST::updateBeadCount;
    using ST::updateIsCentral;
};

// WallToolPaths' protected post-processing steps.
class Walls : public Slic3r::Arachne::WallToolPaths
{
public:
    using WallToolPaths::WallToolPaths;
    using WallToolPaths::removeSmallLines;
    using WallToolPaths::simplifyToolPaths;
    using WallToolPaths::stitchToolPaths;
};

std::string bytes_of(const Toolpaths &t)
{
    Payload p;
    put(p, t);
    return p.bytes;
}

void put_opt(Payload &p, const std::optional<coord_t> &v)
{
    p.boolean(bool(v));
    if (v)
        p.i64(*v);
}

// ---------------------------------------------------------------- ANGR on copies

// op 0: collapseSmallEdges with several snap distances on the graph after construction.
Payload angr_collapse(const std::string &name, Graph &g)
{
    Payload p;
    p.str(name).u8(0);
    put_graph(p, g);
    const std::vector<coord_t> snaps = {5, 2000};
    p.u64(snaps.size());
    for (coord_t snap : snaps) {
        Graph c;
        copy_graph(g, c);
        c.collapseSmallEdges(snap);
        p.i64(snap);
        put_graph(p, c);
    }
    return p;
}

// op 1: makeRib on the first quads' middle edges, with the quad's ends as the source (as
// constructFromPolygons calls it), each on its own copy.
Payload angr_rib(const std::string &name, Graph &g)
{
    Payload p;
    p.str(name).u8(1);
    put_graph(p, g);
    std::vector<int32_t> picks;
    int32_t              i = 0;
    for (Edge &e : g.edges) {
        if (picks.size() >= 3)
            break;
        if (e.prev && e.next && e.twin)
            picks.push_back(i);
        ++i;
    }
    p.u64(picks.size());
    for (int32_t k : picks) {
        Graph c;
        copy_graph(g, c);
        Edge *e     = edge_at(c, k);
        Edge *start = e;
        while (start->prev)
            start = start->prev;
        Edge *end = e;
        while (end->next)
            end = end->next;
        const Point a = start->from->p, b = end->to->p;
        Edge       *prev = e;
        c.makeRib(prev, a, b);
        p.i32(k);
        put(p, a);
        put(p, b);
        p.i32(edge_index(c, prev));
        put_graph(p, c);
    }
    return p;
}

// op 2: insertNode at the middle of the first central upward edges (as applyTransitions and
// generateExtraRibs call it), each on its own copy of the graph after filterNoncentralRegions.
Payload angr_insert(const std::string &name, Graph &g)
{
    Payload p;
    p.str(name).u8(2);
    put_graph(p, g);
    std::vector<int32_t> picks;
    int32_t              i = 0;
    for (Edge &e : g.edges) {
        if (picks.size() >= 3)
            break;
        if (e.twin && e.data.centralIsSet() && e.data.isCentral() && e.data.type != Slic3r::Arachne::SkeletalTrapezoidationEdge::EdgeType::EXTRA_VD &&
            e.from->data.distance_to_boundary < e.to->data.distance_to_boundary && (e.to->p - e.from->p).cast<int64_t>().squaredNorm() > 4LL * 20000 * 20000)
            picks.push_back(i);
        ++i;
    }
    p.u64(picks.size());
    for (int32_t k : picks) {
        Graph c;
        copy_graph(g, c);
        Edge         *e     = edge_at(c, k);
        const Point   mid   = (e->from->p + e->to->p) / 2;
        const coord_t count = std::min(e->from->data.bead_count, e->to->data.bead_count);
        Edge         *ret   = c.insertNode(e, mid, count);
        p.i32(k);
        put(p, mid);
        p.i64(count).i32(edge_index(c, ret));
        put_graph(p, c);
    }
    return p;
}

// op 3: isLocalMaximum() of every node, on the graph updateBeadCount reads.
Payload angr_maxima(const std::string &name, Graph &g)
{
    Payload p;
    p.str(name).u8(3);
    put_graph(p, g);
    p.u64(g.nodes.size());
    for (Node &n : g.nodes)
        p.boolean(n.isLocalMaximum());
    return p;
}

// op 4: isUpward() and distToGoUp() of each edge and its twin, for the edges generateSegments
// tests (those with a previous and a next edge), on the graph it reads.
Payload angr_upward(const std::string &name, Graph &g)
{
    Payload p;
    p.str(name).u8(4);
    put_graph(p, g);
    std::vector<Edge *>   picks;
    std::vector<int32_t>  index;
    int32_t               i = 0;
    for (Edge &e : g.edges) {
        if (e.prev && e.next) {
            picks.push_back(&e);
            index.push_back(i);
        }
        ++i;
    }
    p.u64(picks.size());
    for (size_t k = 0; k < picks.size(); ++k) {
        Edge *e = picks[k];
        p.i32(index[k]).boolean(e->isUpward());
        put_opt(p, e->distToGoUp());
        put_opt(p, e->twin->distToGoUp());
    }
    return p;
}

// ---------------------------------------------------------------- one case

struct Flags
{
    bool commit_skeletal = false, commit_sub = false, graph = false, commit_graph = false;
};

int g_mismatch = 0;

void run_case(Out &out, const std::string &name, const Polygons &outline, const WallSetup &setup, const Flags &f)
{
    const Slic3r::Arachne::WallToolPathsParams w = wall_params(setup);
    const StArgs                               a = st_args(setup, w);
    const Polygons prepared = prepare_outline(outline, a.smallest_segment, a.allowed_distance, setup.bead_width_0, nullptr);
    if (setup.inset_count < 1 || Slic3r::area(prepared) <= 0)
        return;
    StrategySpec spec;
    spec.args                = a.strat;
    log_take();
    BeadingStrategyPtr strat = make_strategy(spec);
    Case               sk, gr, ws;
    Payload            sc;
    sc.str(name);
    put(sc, setup);
    put(sc, prepared);
    put(sc, spec);
    sc.f64(strat->getTransitioningAngle()).i64(a.discretization).i64(a.filter_dist).i64(a.filter_deviation).i64(a.propagation_dist);
    Staged st(prepared, *strat, strat->getTransitioningAngle(), a.discretization, a.filter_dist, a.filter_deviation, a.propagation_dist);
    put(sc, log_take());
    sk.add("ANSC", sc);
    auto stage = [&](uint8_t i) {
        Payload p;
        p.u8(i);
        put_graph(p, st.graph);
        put(p, log_take());
        sk.add("ANST", p);
    };
    stage(0);
    if (f.graph) {
        gr.add("ANGR", angr_collapse(name, st.graph));
        gr.add("ANGR", angr_rib(name, st.graph));
    }
    Toolpaths tp;
    st.p_generated_toolpaths = &tp;
    st.updateIsCentral();
    stage(1);
    st.filterCentral(scaled<coord_t>(0.02));
    stage(2);
    if (f.graph)
        gr.add("ANGR", angr_maxima(name, st.graph));
    st.updateBeadCount();
    stage(3);
    st.filterNoncentralRegions();
    stage(4);
    if (f.graph)
        gr.add("ANGR", angr_insert(name, st.graph));
    {
        // generateTransitioningRibs' steps, with its two lists alive until its end.
        Mids mids;
        st.generateTransitionMids(mids);
        stage(5);
        st.filterTransitionMids();
        stage(6);
        Ends ends;
        st.generateAllTransitionEnds(ends);
        stage(7);
        st.applyTransitions(ends);
        stage(8);
    }
    st.generateExtraRibs();
    stage(9);
    if (f.graph)
        gr.add("ANGR", angr_upward(name, st.graph));
    st.generateSegments();
    Payload p10;
    p10.u8(10);
    put(p10, tp);
    put(p10, log_take());
    Toolpaths direct;
    {
        ST d(prepared, *strat, strat->getTransitioningAngle(), a.discretization, a.filter_dist, a.filter_deviation, a.propagation_dist);
        d.generateToolpaths(direct);
    }
    log_take();
    const bool same = bytes_of(direct) == bytes_of(tp);
    if (!same) {
        ++g_mismatch;
        std::clog << "arachne probe: the staged run of " << name << " (" << setup.name << ") differs from generateToolpaths\n";
    }
    p10.boolean(same);
    sk.add("ANST", p10);
    out.put("skeletal", sk, f.commit_skeletal);
    if (f.graph)
        out.put("graph", gr, f.commit_graph);

    // ANWS: WallToolPaths' steps after the trapezoidation, on these toolpaths.
    Walls   walls(outline, setup.bead_width_0, setup.bead_width_x, setup.inset_count, setup.wall_0_inset, setup.layer_height, w);
    Payload s;
    s.str(name);
    put(s, setup);
    put(s, w);
    Toolpaths t = tp;
    put(s, t);
    Walls::stitchToolPaths(t, setup.bead_width_x);
    put(s, t);
    walls.removeSmallLines(t);
    put(s, t);
    Walls::simplifyToolPaths(t, w);
    put(s, t);
    const bool empty = Walls::removeEmptyToolPaths(t);
    put(s, t);
    s.boolean(empty);
    put(s, log_take());
    // A check of the probe's arguments: WallToolPaths::generate's result is these toolpaths without
    // the zero-width insets that separateOutInnerContour takes out.
    {
        Slic3r::Arachne::WallToolPaths whole(outline, setup.bead_width_0, setup.bead_width_x, setup.inset_count, setup.wall_0_inset, setup.layer_height, w);
        const Toolpaths                engine = whole.getToolPaths();
        Toolpaths                      mine;
        for (const VariableWidthLines &inset : t) {
            bool zero = !inset.empty();
            for (const ExtrusionLine &l : inset)
                for (const ExtrusionJunction &j : l.junctions)
                    zero = zero && j.w == 0;
            if (!zero)
                mine.push_back(inset);
        }
        const bool match = bytes_of(engine) == bytes_of(mine);
        if (!match) {
            ++g_mismatch;
            std::clog << "arachne probe: the stepwise WallToolPaths of " << name << " (" << setup.name << ") differs from getToolPaths\n";
        }
        s.boolean(match);
        log_take();
    }
    ws.add("ANWS", s);
    out.put("substeps", ws, f.commit_sub);
}

// ---------------------------------------------------------------- ANIP

Payload anip(Staged &st, const Beading &left, double ratio, const Beading &right, const std::optional<coord_t> &radius)
{
    Payload p;
    put(p, left);
    p.f64(ratio);
    put(p, right);
    put_opt(p, radius);
    put(p, radius ? st.interpolate(left, ratio, right, *radius) : st.interpolate(left, ratio, right));
    return p;
}

void record_interpolate(Out &out)
{
    const WallSetup  s0 = wall_setups().front();
    const StArgs     a  = st_args(s0, wall_params(s0));
    StrategySpec     spec;
    spec.args                = a.strat;
    BeadingStrategyPtr strat = make_strategy(spec);
    const Polygons     tiny  = {rect(0, 0, 2, 1)};
    Staged             st(tiny, *strat, strat->getTransitioningAngle(), a.discretization, a.filter_dist, a.filter_deviation, a.propagation_dist);
    log_take();
    const std::vector<double> ratios = {0., 0.25, 0.5, 0.731, 1.};
    auto                      usable = [](const Beading &l, const Beading &r) {
        // The four-argument form reads past the right beading unless it has at most one location
        // fewer than the left one and the left one is the thicker (as generateSegments calls it).
        return l.total_thickness >= r.total_thickness && r.toolpath_locations.size() + 1 >= l.toolpath_locations.size();
    };
    {
        Case c;
        // A node's two beadings (bead count n and n + 1 at one thickness), as the beading store merges them.
        for (coord_t t : {coord_t(300000), coord_t(600000), coord_t(800000), coord_t(1100000), coord_t(1500000)}) {
            const coord_t n = strat->getOptimalBeadCount(t);
            const Beading lo = strat->compute(t, n), hi = strat->compute(t, n + 1);
            for (double r : ratios)
                c.add("ANIP", anip(st, lo, r, hi, std::nullopt));
        }
        // A top beading over a lower one, with switching radii about the lower one's locations.
        const std::vector<std::pair<coord_t, coord_t>> pairs = {{1200000, 800000}, {900000, 850000}, {1600000, 700000}, {2000000, 1900000}};
        for (auto [tt, tb] : pairs) {
            const Beading top = strat->compute(tt, strat->getOptimalBeadCount(tt)), bottom = strat->compute(tb, strat->getOptimalBeadCount(tb));
            if (!usable(top, bottom))
                continue;
            std::vector<coord_t> radii = {0, tb / 2, tt / 2};
            for (coord_t loc : top.toolpath_locations)
                radii.insert(radii.end(), {loc - 1, loc, loc + 1});
            for (double r : {0., 0.4, 1.})
                for (coord_t rad : radii)
                    c.add("ANIP", anip(st, top, r, bottom, rad));
        }
        out.put("interp", c, true);
    }
    SplitMix g(0xA7A1C500ull);
    for (int block = 0; block < 6; ++block) {
        Case c;
        for (int k = 0; k < 40; ++k) {
            const coord_t tt = 100000 + coord_t(g.below(2500000));
            const coord_t tb = coord_t(g.below(uint64_t(tt)));
            const Beading top = strat->compute(tt, strat->getOptimalBeadCount(tt) + coord_t(g.below(2)));
            const Beading bottom = strat->compute(tb, strat->getOptimalBeadCount(tb) + coord_t(g.below(2)));
            const double  r = double(g.below(1001)) / 1000.;
            if (k % 2 == 0 || !usable(top, bottom))
                c.add("ANIP", anip(st, top, r, bottom, std::nullopt));
            else
                c.add("ANIP", anip(st, top, r, bottom, coord_t(g.below(uint64_t(tt / 2 + 1)))));
        }
        out.put("interp", c, block == 0);
    }
    log_take();
}

} // namespace

bool record_skeletal(Out &out, std::string &error)
{
    const std::vector<Shape>     own    = shapes();
    const std::vector<Shape>     mesh   = mesh_slices(error);
    const std::vector<WallSetup> setups = wall_setups();
    if (!error.empty())
        return false;
    size_t i = 0;
    for (const Shape &s : own) {
        // Every own shape's case is marked; the families' shares take the ones that fit, in order.
        Flags f;
        f.commit_skeletal = true;
        f.commit_sub      = true;
        f.graph           = i < 10;
        f.commit_graph    = true;
        run_case(out, s.name, s.polygons, setups[0], f);
        if (i < 12)
            for (size_t k = 1; k + 1 < setups.size(); ++k) {
                Flags other;
                other.commit_skeletal = other.commit_sub = i < 4;
                run_case(out, s.name, s.polygons, setups[k], other);
            }
        ++i;
    }
    for (const Shape &s : mesh)
        run_case(out, s.name, s.polygons, setups[0], Flags{});
    record_interpolate(out);
    if (g_mismatch != 0)
        std::clog << "arachne probe: " << g_mismatch << " stepwise runs differ from the engine's own\n";
    return true;
}

} // namespace probe::arachne
