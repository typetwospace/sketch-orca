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
// The arachne probe's shared parts (common.hpp): the committed set's budget, the value writers, the
// graph's writer and copy, the log capture, the beading strategies, the shapes and the outline
// preparation of WallToolPaths::generate. Recorded by orca-o3-probe-arachne, 2026-11-13.
#include "common.hpp"

#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <filesystem>
#include <iostream>
#include <sstream>
#include <unordered_map>

#include <boost/log/attributes/value_extraction.hpp>
#include <boost/log/core.hpp>
#include <boost/log/sinks/basic_sink_backend.hpp>
#include <boost/log/sinks/sync_frontend.hpp>
#include <boost/log/trivial.hpp>
#include <boost/make_shared.hpp>
#include <boost/shared_ptr.hpp>

#include "libslic3r/Arachne/BeadingStrategy/BeadingStrategyFactory.hpp"
#include "libslic3r/Arachne/BeadingStrategy/DistributedBeadingStrategy.hpp"
#include "libslic3r/Arachne/BeadingStrategy/LimitedBeadingStrategy.hpp"
#include "libslic3r/Arachne/BeadingStrategy/OuterWallInsetBeadingStrategy.hpp"
#include "libslic3r/Arachne/BeadingStrategy/RedistributeBeadingStrategy.hpp"
#include "libslic3r/Arachne/BeadingStrategy/WideningBeadingStrategy.hpp"
#include "libslic3r/ClipperUtils.hpp"
#include "libslic3r/ExPolygon.hpp"
#include "libslic3r/Format/OBJ.hpp"
#include "libslic3r/Geometry.hpp"
#include "libslic3r/TriangleMesh.hpp"
#include "libslic3r/TriangleMeshSlicer.hpp"

namespace probe::arachne {

// ---------------------------------------------------------------- the committed set's budget

namespace {

// Each family's share of the committed file's 512 KB (496,000 bytes together, so the file stays
// under it with HEAD, PRUN and END). The marked cases are taken in order while they fit: a case
// that would pass its family's share goes to the full set only, and a later smaller one may still
// be taken.
const std::map<std::string, size_t> &budgets()
{
    static const std::map<std::string, size_t> b = {
        {"beading", 62000}, {"linalg", 8000},     {"line", 40000},     {"areaerr", 8000},  {"squaregrid", 24000},
        {"pointgrid", 14000}, {"linegrid", 16000}, {"stitch", 30000},  {"graph", 50000},   {"skeletal", 120000},
        {"interp", 14000},  {"substeps", 36000},  {"helpers", 30000},  {"walls", 40000},   {"params", 4000},
    };
    return b;
}

} // namespace

Out::Out(Sets &sets) : sets_(sets), left_(budgets()) {}

void Out::put(const std::string &family, const Case &c, bool committable)
{
    auto it      = left_.find(family);
    bool commit  = committable && it != left_.end() && c.bytes <= it->second;
    for (const auto &[kind, bytes] : c.records) {
        char k[5] = {kind[0], kind[1], kind[2], kind[3], 0};
        Payload p;
        p.bytes = bytes;
        sets_.full.add(k, p);
        if (commit)
            sets_.committed.add(k, p);
    }
    if (commit) {
        it->second -= c.bytes;
        used_[family] += c.bytes;
    }
}

std::string Out::report() const
{
    std::ostringstream s;
    for (const auto &[f, n] : used_)
        s << f << " " << n << "; ";
    return s.str();
}

// ---------------------------------------------------------------- values

void put(Payload &p, const Point &q) { p.i64(q.x()).i64(q.y()); }
void put(Payload &p, const Points &pts)
{
    p.u64(pts.size());
    for (const Point &q : pts)
        put(p, q);
}
void put(Payload &p, const Polygons &polys)
{
    p.u64(polys.size());
    for (const Polygon &q : polys)
        put(p, q.points);
}
void put(Payload &p, const ExtrusionJunction &j)
{
    put(p, j.p);
    p.i64(j.w).u64(uint64_t(j.perimeter_index));
}
void put(Payload &p, const ExtrusionLine &l)
{
    p.u64(uint64_t(l.inset_idx)).boolean(l.is_odd).boolean(l.is_closed);
    p.u64(l.junctions.size());
    for (const ExtrusionJunction &j : l.junctions)
        put(p, j);
}
void put(Payload &p, const VariableWidthLines &ls)
{
    p.u64(ls.size());
    for (const ExtrusionLine &l : ls)
        put(p, l);
}
void put(Payload &p, const Toolpaths &t)
{
    p.u64(t.size());
    for (const VariableWidthLines &ls : t)
        put(p, ls);
}
void put_i64s(Payload &p, const std::vector<coord_t> &v)
{
    p.u64(v.size());
    for (coord_t x : v)
        p.i64(x);
}
void put(Payload &p, const Beading &b)
{
    p.i64(b.total_thickness);
    put_i64s(p, b.bead_widths);
    put_i64s(p, b.toolpath_locations);
    p.i64(b.left_over);
}

// ---------------------------------------------------------------- the graph

namespace {

struct GraphIndex
{
    std::unordered_map<const Node *, int32_t> nodes;
    std::unordered_map<const Edge *, int32_t> edges;
    explicit GraphIndex(const Graph &g)
    {
        int32_t i = 0;
        for (const Node &n : g.nodes)
            nodes.emplace(&n, i++);
        i = 0;
        for (const Edge &e : g.edges)
            edges.emplace(&e, i++);
    }
    int32_t node(const Node *n) const
    {
        if (!n)
            return -1;
        auto it = nodes.find(n);
        return it == nodes.end() ? -2 : it->second;
    }
    int32_t edge(const Edge *e) const
    {
        if (!e)
            return -1;
        auto it = edges.find(e);
        return it == edges.end() ? -2 : it->second;
    }
};

} // namespace

void put_graph(Payload &p, Graph &g)
{
    const GraphIndex ix(g);
    p.u64(g.nodes.size()).u64(g.edges.size());
    for (Node &n : g.nodes) {
        put(p, n.p);
        p.i64(n.data.distance_to_boundary).i64(n.data.bead_count).f32(n.data.transition_ratio).i32(ix.edge(n.incident_edge));
        std::shared_ptr<Slic3r::Arachne::SkeletalTrapezoidationJoint::BeadingPropagation> b = n.data.hasBeading() ? n.data.getBeading() : nullptr;
        p.boolean(b != nullptr);
        if (b) {
            put(p, b->beading);
            p.i64(b->dist_to_bottom_source).i64(b->dist_from_top_source).boolean(b->is_upward_propagated_only);
        }
    }
    for (Edge &e : g.edges) {
        p.u8(uint8_t(e.data.type));
        p.u8(uint8_t(int8_t(e.data.centralIsSet() ? (e.data.isCentral() ? 1 : 0) : -1)));
        p.i32(ix.node(e.from)).i32(ix.node(e.to)).i32(ix.edge(e.twin)).i32(ix.edge(e.next)).i32(ix.edge(e.prev));
        auto mids = e.data.hasTransitions(true) ? e.data.getTransitions() : nullptr;
        p.boolean(mids != nullptr);
        if (mids) {
            p.u64(mids->size());
            for (const auto &m : *mids)
                p.i64(m.pos).i32(m.lower_bead_count).i64(m.feature_radius);
        }
        auto ends = e.data.hasTransitionEnds(true) ? e.data.getTransitionEnds() : nullptr;
        p.boolean(ends != nullptr);
        if (ends) {
            p.u64(ends->size());
            for (const auto &m : *ends)
                p.i64(m.pos).i32(m.lower_bead_count).boolean(m.is_lower_end);
        }
        auto js = e.data.hasExtrusionJunctions(true) ? e.data.getExtrusionJunctions() : nullptr;
        p.boolean(js != nullptr);
        if (js) {
            p.u64(js->size());
            for (const auto &j : *js)
                put(p, j);
        }
    }
}

void copy_graph(const Graph &g, Graph &out)
{
    const GraphIndex   ix(g);
    std::vector<Node *> nn;
    std::vector<Edge *> ee;
    out.nodes.clear();
    out.edges.clear();
    for (const Node &n : g.nodes) {
        out.nodes.emplace_back(n.data, n.p);
        nn.push_back(&out.nodes.back());
    }
    for (const Edge &e : g.edges) {
        out.edges.emplace_back(e.data);
        ee.push_back(&out.edges.back());
    }
    auto N = [&](const Node *n) -> Node * { const int32_t i = ix.node(n); return i < 0 ? nullptr : nn[size_t(i)]; };
    auto E = [&](const Edge *e) -> Edge * { const int32_t i = ix.edge(e); return i < 0 ? nullptr : ee[size_t(i)]; };
    size_t i = 0;
    for (const Node &n : g.nodes)
        nn[i++]->incident_edge = E(n.incident_edge);
    i = 0;
    for (const Edge &e : g.edges) {
        Edge *c = ee[i++];
        c->from = N(e.from);
        c->to   = N(e.to);
        c->twin = E(e.twin);
        c->next = E(e.next);
        c->prev = E(e.prev);
    }
}

int32_t edge_index(const Graph &g, const Edge *e)
{
    if (!e)
        return -1;
    int32_t i = 0;
    for (const Edge &x : g.edges) {
        if (&x == e)
            return i;
        ++i;
    }
    return -2;
}

int32_t node_index(const Graph &g, const Node *n)
{
    if (!n)
        return -1;
    int32_t i = 0;
    for (const Node &x : g.nodes) {
        if (&x == n)
            return i;
        ++i;
    }
    return -2;
}

Edge *edge_at(Graph &g, int32_t i)
{
    for (Edge &x : g.edges)
        if (i-- == 0)
            return &x;
    return nullptr;
}

// ---------------------------------------------------------------- logs

namespace {

class Capture : public boost::log::sinks::basic_sink_backend<boost::log::sinks::synchronized_feeding>
{
public:
    void consume(const boost::log::record_view &rec)
    {
        int sev = -1;
        if (auto s = boost::log::extract<boost::log::trivial::severity_level>("Severity", rec))
            sev = int(*s);
        std::string msg;
        if (auto m = boost::log::extract<std::string>("Message", rec))
            msg = *m;
        records.emplace_back(sev, std::move(msg));
    }
    std::vector<std::pair<int, std::string>> records;
};

boost::shared_ptr<boost::log::sinks::synchronous_sink<Capture>> g_sink;
std::ostringstream                                             g_cerr;
std::streambuf                                                *g_cerr_old = nullptr;

} // namespace

void log_start()
{
    g_sink = boost::make_shared<boost::log::sinks::synchronous_sink<Capture>>();
    boost::log::core::get()->add_sink(g_sink);
    g_cerr_old = std::cerr.rdbuf(g_cerr.rdbuf());
}

void log_stop()
{
    if (g_cerr_old)
        std::cerr.rdbuf(g_cerr_old);
    g_cerr_old = nullptr;
    if (g_sink)
        boost::log::core::get()->remove_sink(g_sink);
    g_sink.reset();
}

LogText log_take()
{
    LogText out;
    if (g_sink) {
        auto backend = g_sink->locked_backend();
        out.records.swap(backend->records);
    }
    out.cerr = g_cerr.str();
    g_cerr.str(std::string());
    g_cerr.clear();
    return out;
}

void put(Payload &p, const LogText &l)
{
    p.u64(l.records.size());
    for (const auto &[sev, msg] : l.records)
        p.u8(uint8_t(sev)).str(msg);
    p.str(l.cerr);
}

// ---------------------------------------------------------------- beading strategies

BeadingStrategyPtr make_strategy(const StrategySpec &s)
{
    using namespace Slic3r::Arachne;
    if (s.factory) {
        const FactoryArgs &a = s.args;
        return BeadingStrategyFactory::makeStrategy(a.outer, a.inner, a.transition_length, a.transitioning_angle, a.print_thin_walls, a.min_bead_width,
                                                    a.min_feature_size, a.split, a.add, a.max_bead_count, a.outer_wall_offset,
                                                    a.inward_distributed_center_wall_count, a.minimum_variable_line_ratio);
    }
    BeadingStrategyPtr ret;
    for (const Layer &l : s.chain) {
        switch (l.kind) {
        case 0: ret = std::make_unique<DistributedBeadingStrategy>(l.a, l.b, l.f, l.g, l.h, l.radius); break;
        case 1: ret = std::make_unique<RedistributeBeadingStrategy>(l.a, l.f, std::move(ret)); break;
        case 2: ret = std::make_unique<WideningBeadingStrategy>(std::move(ret), l.a, l.b); break;
        case 3: ret = std::make_unique<OuterWallInsetBeadingStrategy>(l.a, std::move(ret)); break;
        default: ret = std::make_unique<LimitedBeadingStrategy>(l.a, std::move(ret)); break;
        }
    }
    return ret;
}

void put(Payload &p, const StrategySpec &s)
{
    p.u8(s.factory ? 1 : 0);
    if (s.factory) {
        const FactoryArgs &a = s.args;
        p.i64(a.outer).i64(a.inner).i64(a.transition_length).f32(a.transitioning_angle).boolean(a.print_thin_walls);
        p.i64(a.min_bead_width).i64(a.min_feature_size).f64(a.split).f64(a.add).i64(a.max_bead_count).i64(a.outer_wall_offset);
        p.i32(a.inward_distributed_center_wall_count).f64(a.minimum_variable_line_ratio);
        return;
    }
    p.u64(s.chain.size());
    for (const Layer &l : s.chain) {
        p.u8(l.kind);
        switch (l.kind) {
        case 0: p.i64(l.a).i64(l.b).f64(l.f).f64(l.g).f64(l.h).i32(l.radius); break;
        case 1: p.i64(l.a).f64(l.f); break;
        case 2: p.i64(l.a).i64(l.b); break;
        default: p.i64(l.a); break;
        }
    }
}

FactoryArgs wall_args(double ext_spacing, double inner_spacing, double nozzle, size_t inset_count, coord_t wall_0_inset)
{
    FactoryArgs a;
    a.outer                                = scaled<coord_t>(ext_spacing);
    a.inner                                = scaled<coord_t>(inner_spacing);
    a.transition_length                    = scaled<coord_t>(1.0 * nozzle);
    a.transitioning_angle                  = float(Slic3r::Geometry::deg2rad(10.f));
    a.print_thin_walls                     = true;
    a.min_bead_width                       = scaled<coord_t>(float(0.85 * nozzle));
    a.min_feature_size                     = scaled<coord_t>(float(0.25 * nozzle));
    a.split                                = std::clamp(2. * 0.85 * nozzle / (ext_spacing + 0.04) - 1., 0.01, 0.99);
    a.add                                  = std::clamp(0.85 * nozzle / (inner_spacing + 0.04), 0.01, 0.99);
    a.max_bead_count                       = coord_t(2 * inset_count);
    a.outer_wall_offset                    = wall_0_inset;
    a.inward_distributed_center_wall_count = 1;
    a.minimum_variable_line_ratio          = 0.5;
    return a;
}

// ---------------------------------------------------------------- shapes

namespace {

coord_t S(double mm) { return coord_t(std::llround(mm * 1000000.)); }

} // namespace

Polygon rect(double x0, double y0, double x1, double y1)
{
    Polygon p;
    p.points = {Point(S(x0), S(y0)), Point(S(x1), S(y0)), Point(S(x1), S(y1)), Point(S(x0), S(y1))};
    return p;
}

Polygon circle(double cx, double cy, double r, int n, bool hole)
{
    Polygon p;
    for (int i = 0; i < n; ++i) {
        const double a = 2. * M_PI * double(i) / double(n);
        p.points.emplace_back(S(cx + r * std::cos(a)), S(cy + r * std::sin(a)));
    }
    if (hole)
        std::reverse(p.points.begin(), p.points.end());
    return p;
}

Polygon star(SplitMix &g, double cx, double cy, double r_min, double r_max, int n)
{
    Polygon p;
    for (int i = 0; i < n; ++i) {
        const double a = 2. * M_PI * (double(i) + 0.6 * double(g.below(1000)) / 1000.) / double(n);
        const double r = r_min + (r_max - r_min) * double(g.below(1000)) / 1000.;
        p.points.emplace_back(S(cx + r * std::cos(a)), S(cy + r * std::sin(a)));
    }
    return p;
}

namespace {

Polygons unite(const Polygons &a) { return Slic3r::union_(a); }

Polygon poly(std::initializer_list<std::pair<double, double>> pts)
{
    Polygon p;
    for (const auto &[x, y] : pts)
        p.points.emplace_back(S(x), S(y));
    return p;
}

} // namespace

std::vector<Shape> shapes()
{
    std::vector<Shape> out;
    auto               add = [&](const std::string &name, Polygons polys) { out.push_back({name, std::move(polys)}); };
    // Small shapes first: the committed set takes cases from the front.
    add("rect 10x0.3", {rect(0, 0, 10, 0.3)});
    add("rect 10x0.6", {rect(0, 0, 10, 0.6)});
    add("rect 8x1", {rect(0, 0, 8, 1.0)});
    add("wedge 0.2 to 3 over 20", {poly({{0, -0.1}, {20, -1.5}, {20, 1.5}, {0, 0.1}})});
    add("rect 6x2", {rect(0, 0, 6, 2)});
    add("square 5 hole 3", {rect(0, 0, 5, 5), [] { Polygon h = rect(1, 1, 4, 4); h.reverse(); return h; }()});
    add("L 6x6 width 1.3", unite({rect(0, 0, 6, 1.3), rect(0, 0, 1.3, 6)}));
    add("diamond 12x1.6", {poly({{0, 0}, {6, -0.8}, {12, 0}, {6, 0.8}})});
    add("rect 10x1.25", {rect(0, 0, 10, 1.25)});
    add("rect 10x0.85", {rect(0, 0, 10, 0.85)});
    add("circle r3 n48", {circle(0, 0, 3, 48)});
    add("T 8x6", unite({rect(0, 5, 8, 6.2), rect(3.4, 0, 4.6, 6)}));
    add("annulus 5 3.6 n64", {circle(0, 0, 5, 64), circle(0, 0, 3.6, 64, true)});
    add("two islands", {rect(0, 0, 3, 0.9), rect(5, 0, 9, 2.2)});
    add("comb", unite({rect(0, 0, 12, 1.5), rect(0, 0, 0.5, 5), rect(2, 0, 2.7, 5), rect(4, 0, 5.1, 5), rect(6.5, 0, 6.85, 5), rect(9, 0, 10.6, 5)}));
    add("zigzag strip", {poly({{0, 0}, {2, 2}, {4, 0}, {6, 2}, {8, 0}, {8, 0.7}, {6, 2.7}, {4, 0.7}, {2, 2.7}, {0, 0.7}})});
    add("sliver triangle", {poly({{0, 0}, {15, 0}, {15, 0.9}})});
    add("square 20", {rect(0, 0, 20, 20)});
    add("square 20 hole 16", {rect(0, 0, 20, 20), [] { Polygon h = rect(2, 2, 18, 18); h.reverse(); return h; }()});
    add("circle r10 n180", {circle(0, 0, 10, 180)});
    {
        SplitMix g(0xA7AC4E01ull);
        for (int i = 0; i < 6; ++i) {
            const int    n  = 5 + int(g.below(30));
            const double r0 = 0.4 + double(g.below(300)) / 100.;
            const double r1 = r0 + 0.3 + double(g.below(600)) / 100.;
            Polygon      p  = star(g, 0, 0, r0, r1, n);
            add("star " + std::to_string(i), unite({p}));
        }
    }
    add("concave letter", unite({rect(0, 0, 1.1, 8), rect(0, 7, 5, 8), rect(0, 3.5, 3.6, 4.4), rect(0, 0, 5, 0.9)}));
    add("ring of circles", unite({circle(0, 0, 1.2, 40), circle(2.2, 0, 1.2, 40), circle(1.1, 1.9, 1.2, 40)}));
    return out;
}

std::vector<Shape> mesh_slices(std::string &error)
{
    std::vector<Shape> out;
    char              *pgm = nullptr;
    if (_get_pgmptr(&pgm) != 0 || pgm == nullptr) {
        error = "cannot find the probe's own path";
        return out;
    }
    const std::filesystem::path data = std::filesystem::path(pgm).parent_path().parent_path() / "src" / "tests" / "data";
    const char *names[] = {"A.obj", "V.obj", "frog_legs.obj", "extruder_idler.obj", "ipadstand.obj", "cube_with_concave_hole.obj", "two_hollow_squares.obj",
                           "bridge.obj", "overhang.obj", "small_dorito.obj", "sloping_hole.obj", "pyramid.obj"};
    for (const char *name : names) {
        Slic3r::TriangleMesh mesh;
        Slic3r::ObjInfo      info;
        std::string          message;
        const std::string    path = (data / name).string();
        if (!Slic3r::load_obj(path.c_str(), &mesh, info, message)) {
            error = "cannot read " + path + ": " + message;
            return out;
        }
        const Slic3r::BoundingBoxf3 bb = mesh.bounding_box();
        for (double t : {0.37, 0.61}) {
            const float                     z = float(bb.min.z() + t * (bb.max.z() - bb.min.z()));
            Slic3r::MeshSlicingParamsEx      params;
            std::vector<Slic3r::ExPolygons> layers = Slic3r::slice_mesh_ex(mesh.its, {z}, params);
            Polygons                        polys  = layers.empty() ? Polygons() : Slic3r::to_polygons(layers.front());
            if (!polys.empty())
                out.push_back({std::string(name) + " at " + std::to_string(t), std::move(polys)});
        }
    }
    return out;
}

// ---------------------------------------------------------------- WallToolPaths' outline preparation

void run_step(PrepStep &s)
{
    s.out = s.in;
    switch (s.op) {
    case 0: Slic3r::Arachne::simplify(s.out, s.a, s.b); break;
    case 1: Slic3r::Arachne::fixSelfIntersections(coord_t(s.a), s.out); break;
    case 2: Slic3r::Arachne::removeDegenerateVerts(s.out); break;
    case 3: Slic3r::Arachne::removeColinearEdges(s.out, s.f); break;
    default: Slic3r::Arachne::removeSmallAreas(s.out, s.f, s.holes); break;
    }
}

Polygons prepare_outline(const Polygons &outline, coord_t smallest_segment, coord_t allowed_distance, coord_t bead_width_0, std::vector<PrepStep> *steps)
{
    const coord_t epsilon_offset    = (allowed_distance / 2) - 1;
    const double  small_area_length = static_cast<double>(bead_width_0) / 2.;
    Polygons      cur = Slic3r::offset(Slic3r::offset(Slic3r::offset(outline, -epsilon_offset), epsilon_offset * 2), -epsilon_offset);
    auto          run = [&](PrepStep s) {
        s.in = cur;
        run_step(s);
        cur = s.out;
        if (steps)
            steps->push_back(std::move(s));
    };
    run({0, smallest_segment, allowed_distance});
    run({1, epsilon_offset});
    run({2});
    run({3, 0, 0, 0.005});
    run({1, epsilon_offset});
    run({2});
    run({4, 0, 0, small_area_length * small_area_length, false});
    return Slic3r::union_(cur);
}

} // namespace probe::arachne
