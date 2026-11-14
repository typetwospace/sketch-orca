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
// The voronoi probe's records of voronoi-3 (parity/sop1/voronoi.md, VRVD to VRDZ): whole diagrams
// from boost::polygon::construct_voronoi and from Slic3r::Geometry::VoronoiDiagram's point and
// mixed overloads, VoronoiUtils' vertex helpers, Arachne's PolygonsPointIndex and
// PolygonsSegmentIndex, the source and cell-range lookups on every cell of raw diagrams (no repair,
// so the diagram is the builder's alone) and discretize_parabola on the parabolic edges Arachne
// discretizes. VoronoiUtils' templates are reached through the instantiations VoronoiUtils.cpp
// makes (Lines::iterator and the PolygonsSegmentIndex const_iterator). Written by
// orca-o3-probe-voronoi, 2026-11-13.
#include "common.hpp"

#include <cmath>
#include <functional>
#include <limits>
#include <type_traits>

#include <boost/polygon/polygon.hpp>

#include "libslic3r/Arachne/utils/PolygonsPointIndex.hpp"
#include "libslic3r/Arachne/utils/PolygonsSegmentIndex.hpp"
#include "libslic3r/Geometry/Voronoi.hpp"
#include "libslic3r/Geometry/VoronoiUtils.hpp"

namespace probe::vr {

namespace {

using namespace Slic3r;
using Slic3r::Geometry::VoronoiDiagram;
using Slic3r::Geometry::VoronoiUtils;
using PPI = Slic3r::Arachne::PolygonsPointIndex;
using PSI = Slic3r::Arachne::PolygonsSegmentIndex;
namespace bp = boost::polygon;

// ---------------------------------------------------------------- VRVD

void record_boost_diagrams(Sets &sets)
{
    Budget b{3, 2000};
    for (const SiteSet &ss : site_sets()) {
        std::vector<bp::point_data<bpd::int32>>   pts;
        std::vector<bp::segment_data<bpd::int32>> segs;
        for (const Pt32 &q : ss.points)
            pts.emplace_back(q.x, q.y);
        for (const Seg32 &s : ss.segments)
            segs.emplace_back(bp::point_data<bpd::int32>(s.a.x, s.a.y), bp::point_data<bpd::int32>(s.b.x, s.b.y));
        BVD     vd;
        uint8_t entry;
        if (segs.empty()) {
            entry = 0;
            bp::construct_voronoi(pts.begin(), pts.end(), &vd);
        } else if (pts.empty()) {
            entry = 1;
            bp::construct_voronoi(segs.begin(), segs.end(), &vd);
        } else {
            entry = 2;
            bp::construct_voronoi(pts.begin(), pts.end(), segs.begin(), segs.end(), &vd);
        }
        Payload p;
        p.u8(entry);
        put_siteset(p, ss);
        put_diagram(p, vd);
        put_record(sets, "VRVD", p, b.take(p, ss.hand));
    }
}

// ---------------------------------------------------------------- VRVG

void record_slic3r_point_diagrams(Sets &sets)
{
    Budget b{4, 2000};
    for (const SiteSet &ss : site_sets()) {
        if (ss.points.empty() || ss.points.size() + 3 * ss.segments.size() > 60)
            continue;
        Points pts;
        Lines  lines;
        for (const Pt32 &q : ss.points)
            pts.emplace_back(q.x, q.y);
        for (const Seg32 &s : ss.segments)
            lines.emplace_back(Point(s.a.x, s.a.y), Point(s.b.x, s.b.y));
        VoronoiDiagram vd;
        uint8_t        entry;
        if (lines.empty()) {
            entry = 0;
            vd.construct_voronoi(pts.begin(), pts.end());
        } else {
            entry = 2;
            vd.construct_voronoi(pts.begin(), pts.end(), lines.begin(), lines.end());
        }
        Payload p;
        p.u8(entry);
        put_siteset(p, ss);
        p.u8(uint8_t(vd.get_issue_type())).u8(uint8_t(vd.get_state())).boolean(vd.is_valid());
        p.u64(vd.num_vertices()).u64(vd.num_edges()).u64(vd.num_cells());
        put_diagram(p, vd);
        put_record(sets, "VRVG", p, b.take(p, ss.hand));
    }
}

// ---------------------------------------------------------------- VRUT

void record_vertex_utils(Sets &sets)
{
    Budget b{120, 96};
    auto   rec = [&](double x, double y, double angle, uint32_t color, bool hand) {
        VoronoiDiagram::vertex_type v(x, y);
        v.color(color);
        Payload p;
        p.f64(x).f64(y).f64(angle).u32(color);
        const bool finite = VoronoiUtils::is_finite(v);
        p.boolean(finite);
        p.boolean(VoronoiUtils::is_in_range<int64_t>(x)).boolean(VoronoiUtils::is_in_range<int64_t>(y));
        p.boolean(VoronoiUtils::is_in_range<int32_t>(x)).boolean(VoronoiUtils::is_in_range<int32_t>(y));
        p.boolean(VoronoiUtils::is_in_range<int64_t>(v)).boolean(VoronoiUtils::is_in_range<int32_t>(v));
        // to_point rounds with llround; it is called on vertices well inside the 64-bit range only.
        const bool safe = finite && std::fabs(x) < 9.0e18 && std::fabs(y) < 9.0e18;
        p.boolean(safe);
        if (safe) {
            const Vec2i64 q = VoronoiUtils::to_point(v);
            const Vec2i64 r = VoronoiUtils::to_point(&v);
            p.i64(q.x()).i64(q.y()).i64(r.x()).i64(r.y());
        }
        if (finite) {
            const VoronoiDiagram::vertex_type w = VoronoiUtils::make_rotated_vertex(v, angle);
            p.f64(w.x()).f64(w.y()).u32(uint32_t(w.color()));
        }
        put_record(sets, "VRUT", p, b.take(p, hand));
    };
    const double inf = std::numeric_limits<double>::infinity();
    rec(0.0, 0.0, 0.0, 0, true);
    rec(0.5, -0.5, 1.0, 3, true);
    rec(1.5, 2.5, -1.0, 1, true);
    rec(-2.5, -1.5, PI / 6, 7, true);
    rec(2147483647.0, -2147483648.0, PI / 5, 0, true);
    rec(2147483647.5, -2147483648.5, PI / 7, 0, true);
    rec(9.2233720368547758e18, -9.2233720368547758e18, PI / 11, 0, true);
    rec(1e19, 1.0, -PI / 6, 0, true);
    rec(inf, 0.0, 0.5, 0, true);
    rec(0.0, -inf, 0.5, 0, true);
    rec(std::numeric_limits<double>::quiet_NaN(), 0.0, 0.5, 0, true);
    Gen g(0x07Fu);
    for (int i = 0; i < 400; ++i) {
        const double scale  = std::pow(10.0, double(g.range(0, 12)));
        const double x      = g.real(-1., 1.) * scale;
        const double y      = g.chance(10) ? std::floor(x) + 0.5 : g.real(-1., 1.) * scale;
        const double angles[] = {PI / 6, -PI / 6, PI / 5, -PI / 5, PI / 7, -PI / 7, PI / 11, -PI / 11, g.real(-4., 4.)};
        rec(x, y, angles[g.below(9)], uint32_t(g.below(5)), false);
    }
}

// ---------------------------------------------------------------- VRPI

void record_point_index(Sets &sets)
{
    Budget b{3, 4000};
    {
        const PPI empty;
        Payload   p;
        p.u8(0);
        put_pt(p, empty.p());
        p.boolean(empty.initialized());
        put_record(sets, "VRPI", p, true);
    }
    Gen g(0x9917u);
    for (const Shape &s : shapes()) {
        const Polygons polys = arachne_polygons(s.ex);
        const size_t   n     = segment_count(polys);
        if (n == 0 || n > 400)
            continue;
        Payload p;
        p.u8(1);
        put_polys(p, polys);
        p.u32(uint32_t(n));
        for (unsigned poly = 0; poly < polys.size(); ++poly)
            for (unsigned point = 0; point < polys[poly].size(); ++point) {
                const PPI   i(&polys, poly, point);
                const PSI   si(&polys, poly, point);
                const auto  seg = Slic3r::Arachne::PolygonsPointIndexSegmentLocator()(i);
                const Point loc = Slic3r::Arachne::PathsPointIndexLocator<Polygons>()(i);
                p.u32(poly).u32(point);
                put_pt(p, i.p());
                p.u32(i.next().point_idx).u32(i.prev().point_idx);
                put_pt(p, si.from());
                put_pt(p, si.to());
                put_pt(p, seg.first);
                put_pt(p, seg.second);
                put_pt(p, loc);
                p.u64(uint64_t(std::hash<PPI>()(i)));
                const unsigned opoly  = unsigned(g.below(polys.size()));
                const unsigned opoint = unsigned(g.below(polys[opoly].size()));
                const PPI      o(&polys, opoly, opoint);
                p.u32(opoly).u32(opoint).boolean(i < o).boolean(i == o).boolean(i != o);
            }
        put_record(sets, "VRPI", p, b.take(p, s.hand));
    }
}

// ---------------------------------------------------------------- VRCR

// The cells of a raw diagram, each with its lookups: u32 cell index, u8 contains segment, u8
// contains point, then for a segment cell the source segment (two points for Lines, polygon and
// point index for PolygonsSegmentIndex) and compute_segment_cell_range (start, end, i32 edge
// begin, i32 edge end, u8 valid), for a point cell the source point, and for PolygonsSegmentIndex
// get_source_point_index and compute_point_cell_range (u8 called, then source point, edges, valid).
template<typename It> struct CellLookups;

template<> struct CellLookups<Lines::iterator>
{
    static void segment(Payload &p, const Line &l)
    {
        put_pt(p, l.a);
        put_pt(p, l.b);
    }
};

template<> struct CellLookups<std::vector<PSI>::const_iterator>
{
    static void segment(Payload &p, const PSI &s) { p.u32(s.poly_idx).u32(s.point_idx); }
};

// Whether every edge around the cell is finite: compute_point_cell_range reads both vertices of
// every edge once its first edge passes, so the probe calls it only on such cells (as Arachne's
// input, without collinear points, gives it).
bool all_finite(const VoronoiDiagram::cell_type &cell)
{
    const VoronoiDiagram::edge_type *e = cell.incident_edge();
    do {
        if (e->is_infinite())
            return false;
        e = e->next();
    } while (e != cell.incident_edge());
    return true;
}

template<typename It> void put_cells(Payload &p, const VoronoiDiagram &vd, It begin, It end, bool psi)
{
    const auto *e0 = vd.edges().data();
    uint32_t    n  = 0;
    for (const auto &c : vd.cells())
        n += c.is_degenerate() ? 0 : 1;
    p.u32(n);
    for (size_t ci = 0; ci < vd.cells().size(); ++ci) {
        const auto &cell = vd.cells()[ci];
        if (cell.is_degenerate())
            continue;
        p.u32(uint32_t(ci)).boolean(cell.contains_segment()).boolean(cell.contains_point());
        if (cell.contains_segment()) {
            CellLookups<It>::segment(p, VoronoiUtils::get_source_segment(cell, begin, end));
            const auto r = VoronoiUtils::compute_segment_cell_range(cell, begin, end);
            put_pt(p, r.source_segment_start_point);
            put_pt(p, r.source_segment_end_point);
            p.i32(index_of(r.edge_begin, e0)).i32(index_of(r.edge_end, e0)).boolean(r.is_valid());
        } else {
            put_pt(p, VoronoiUtils::get_source_point(cell, begin, end));
            if (psi) {
                if constexpr (std::is_same_v<It, std::vector<PSI>::const_iterator>) {
                    const PPI i = VoronoiUtils::get_source_point_index(cell, begin, end);
                    p.u32(i.poly_idx).u32(i.point_idx);
                    const auto *edge   = cell.incident_edge();
                    const bool  called = edge->is_infinite() || !VoronoiUtils::is_in_range<coord_t>(*edge) || all_finite(cell);
                    p.boolean(called);
                    if (called) {
                        const auto r = VoronoiUtils::compute_point_cell_range(cell, begin, end);
                        put_pt(p, r.source_point);
                        p.i32(index_of(r.edge_begin, e0)).i32(index_of(r.edge_end, e0)).boolean(r.is_valid());
                    }
                }
            }
        }
    }
}

// The lookups that throw: u8 call (0 get_source_segment, 1 get_source_point, 2
// get_source_point_index), u32 cell, u64 range length, then u8 0 when it returned, else the error.
// They are the wrong-kind calls on the first segment cell and the first point cell, and
// get_source_segment on the first segment cell of source index 1 or more with the range cut short
// before that index.
template<typename It> void put_throwing(Payload &p, const VoronoiDiagram &vd, It begin, It end)
{
    struct Call
    {
        uint8_t call;
        size_t  cell;
        size_t  length;
    };
    constexpr bool    psi = std::is_same_v<It, std::vector<PSI>::const_iterator>;
    const size_t      all = size_t(end - begin);
    std::vector<Call> calls;
    bool              seg_done = false, pt_done = false, range_done = false;
    for (size_t ci = 0; ci < vd.cells().size(); ++ci) {
        const auto &cell = vd.cells()[ci];
        if (cell.is_degenerate())
            continue;
        if (cell.contains_segment() && !seg_done) {
            seg_done = true;
            calls.push_back({1, ci, all});
            if (psi)
                calls.push_back({2, ci, all});
        }
        if (cell.contains_point() && !pt_done) {
            pt_done = true;
            calls.push_back({0, ci, all});
        }
        if (cell.contains_segment() && !range_done && cell.source_index() >= 1) {
            range_done = true;
            calls.push_back({0, ci, size_t(cell.source_index())});
        }
    }
    p.u32(uint32_t(calls.size()));
    for (const Call &c : calls) {
        const auto &cell = vd.cells()[c.cell];
        p.u8(c.call).u32(uint32_t(c.cell)).u64(uint64_t(c.length));
        try {
            if (c.call == 0) {
                VoronoiUtils::get_source_segment(cell, begin, begin + c.length);
            } else if (c.call == 1) {
                VoronoiUtils::get_source_point(cell, begin, begin + c.length);
            } else {
                if constexpr (psi)
                    VoronoiUtils::get_source_point_index(cell, begin, begin + c.length);
            }
            p.u8(0);
        } catch (const std::exception &e) {
            put_error(p, e);
        }
    }
}

void record_cell_lookups(Sets &sets)
{
    Budget b{6, 1500};
    for (const Shape &s : shapes()) {
        {
            Lines lines = to_lines(s.ex);
            if (!lines.empty() && lines.size() <= 800) {
                VoronoiDiagram vd;
                vd.construct_voronoi(lines.begin(), lines.end(), false);
                Payload p;
                p.u8(0);
                put_lines(p, lines);
                put_cells(p, vd, lines.begin(), lines.end(), false);
                put_throwing(p, vd, lines.begin(), lines.end());
                put_record(sets, "VRCR", p, b.take(p, s.hand));
            }
        }
        {
            const Polygons polys = arachne_polygons(s.ex);
            const size_t   n     = segment_count(polys);
            if (n != 0 && n <= 800) {
                std::vector<PSI> segs;
                for (unsigned poly = 0; poly < polys.size(); ++poly)
                    for (unsigned point = 0; point < polys[poly].size(); ++point)
                        segs.emplace_back(&polys, poly, point);
                VoronoiDiagram vd;
                vd.construct_voronoi(segs.cbegin(), segs.cend(), false);
                Payload p;
                p.u8(1);
                put_polys(p, polys);
                put_cells(p, vd, segs.cbegin(), segs.cend(), true);
                put_throwing(p, vd, segs.cbegin(), segs.cend());
                put_record(sets, "VRCR", p, b.take(p, s.hand));
            }
        }
    }
    // A diagram of points and segments: its point cells hold SOURCE_CATEGORY_SINGLE_POINT and its
    // segment cells source indexes counted after the points.
    for (const SiteSet &ss : site_sets()) {
        if (!ss.hand || ss.points.empty() || ss.segments.empty())
            continue;
        Points pts;
        Lines  lines;
        for (const Pt32 &q : ss.points)
            pts.emplace_back(q.x, q.y);
        for (const Seg32 &g : ss.segments)
            lines.emplace_back(Point(g.a.x, g.a.y), Point(g.b.x, g.b.y));
        VoronoiDiagram vd;
        vd.construct_voronoi(pts.begin(), pts.end(), lines.begin(), lines.end());
        Payload p;
        p.u8(2);
        put_pts(p, pts);
        put_lines(p, lines);
        uint32_t n = 0;
        for (const auto &c : vd.cells())
            n += c.is_degenerate() ? 0 : 1;
        p.u32(n);
        for (size_t ci = 0; ci < vd.cells().size(); ++ci) {
            const auto &cell = vd.cells()[ci];
            if (cell.is_degenerate())
                continue;
            p.u8(cell.contains_point() ? 1 : 0).u32(uint32_t(ci)).u64(uint64_t(lines.size()));
            // A segment end point cell's index counts the points too, and get_source_point reads the
            // line there unchecked (the assert is off): past the lines that read is undefined, so it is
            // not made (u8 255).
            const bool end_point = cell.source_category() == boost::polygon::SOURCE_CATEGORY_SEGMENT_START_POINT ||
                                   cell.source_category() == boost::polygon::SOURCE_CATEGORY_SEGMENT_END_POINT;
            if (cell.contains_point() && end_point && cell.source_index() >= lines.size()) {
                p.u8(255);
                continue;
            }
            try {
                if (cell.contains_point()) {
                    const Point r = VoronoiUtils::get_source_point(cell, lines.begin(), lines.end());
                    p.u8(0);
                    put_pt(p, r);
                } else {
                    const Line &r = VoronoiUtils::get_source_segment(cell, lines.begin(), lines.end());
                    p.u8(0);
                    put_pt(p, r.a);
                    put_pt(p, r.b);
                }
            } catch (const std::exception &e) {
                put_error(p, e);
            }
        }
        put_record(sets, "VRCR", p, b.take(p, true));
    }
}

// ---------------------------------------------------------------- VRDZ

void record_parabolas(Sets &sets)
{
    Budget      b{50, 400};
    const float angles[] = {float(10. * PI / 180.), float(30. * PI / 180.), float(1. * PI / 180.), 0.f};
    auto        rec      = [&](const Point &source, const PSI &seg, const Point &start, const Point &end, coord_t step, float angle, bool hand) {
        const Points out = VoronoiUtils::discretize_parabola(source, seg, start, end, step, angle);
        Payload      p;
        put_pt(p, source);
        put_pt(p, seg.from());
        put_pt(p, seg.to());
        put_pt(p, start);
        put_pt(p, end);
        p.i64(step).f32(angle);
        put_pts(p, out);
        p.str(take_log());
        put_record(sets, "VRDZ", p, b.take(p, hand));
    };
    take_log();
    for (const Shape &s : shapes()) {
        const Polygons polys = arachne_polygons(s.ex);
        const size_t   n     = segment_count(polys);
        if (n == 0 || n > 800)
            continue;
        std::vector<PSI> segs;
        for (unsigned poly = 0; poly < polys.size(); ++poly)
            for (unsigned point = 0; point < polys[poly].size(); ++point)
                segs.emplace_back(&polys, poly, point);
        VoronoiDiagram vd;
        vd.construct_voronoi(segs.cbegin(), segs.cend(), false);
        size_t taken = 0;
        for (size_t ei = 0; ei < vd.edges().size() && taken < 24; ei += 2) {
            const auto &edge = vd.edges()[ei];
            if (!edge.is_curved() || !edge.is_primary() || edge.is_infinite() || !VoronoiUtils::is_in_range<coord_t>(edge))
                continue;
            const auto *left  = edge.cell();
            const auto *right = edge.twin()->cell();
            if (left->contains_point() == right->contains_point())
                continue;
            const Point  source = VoronoiUtils::get_source_point(*(left->contains_point() ? left : right), segs.cbegin(), segs.cend());
            const PSI   &seg    = VoronoiUtils::get_source_segment(*(left->contains_point() ? right : left), segs.cbegin(), segs.cend());
            const Point  start  = VoronoiUtils::to_point(edge.vertex0()).cast<coord_t>();
            const Point  end    = VoronoiUtils::to_point(edge.vertex1()).cast<coord_t>();
            const coord_t steps[] = {800000, 50000};
            rec(source, seg, start, end, steps[taken % 2], angles[(taken / 2) % 4], s.hand);
            ++taken;
        }
    }
    // Seeded: a segment, a point off it and two ends anywhere near, with the step a fraction of the
    // span so a call makes at most about 150 points.
    Gen g(0x9A2Au);
    for (int i = 0; i < 300; ++i) {
        const coord_t r = g.chance(2) ? 1000 : 10000000;
        Point         a(g.range(-r, r), g.range(-r, r)), c(g.range(-r, r), g.range(-r, r));
        Point         bb(g.range(-r, r), g.range(-r, r));
        if (bb == a)
            bb.x() += 1;
        const Polygons polys{Polygon(Points{a, bb, c})};
        const PSI      seg(&polys, 0, 0);
        const Point    source(g.range(-r, r), g.range(-r, r));
        const Point    start(g.range(-r, r), g.range(-r, r)), end(g.range(-r, r), g.range(-r, r));
        const double   span = (end - start).cast<double>().norm() + 1.;
        const coord_t  step = coord_t(span / double(g.range(1, 150))) + 1;
        const float    angle = g.chance(2) ? angles[g.below(4)] : float(g.real(0., 1.5));
        rec(source, seg, start, end, step, angle, false);
    }
}

} // namespace

void record_diagram(Sets &sets)
{
    record_boost_diagrams(sets);
    record_slic3r_point_diagrams(sets);
    record_vertex_utils(sets);
    record_point_index(sets);
    record_cell_lookups(sets);
    record_parabolas(sets);
}

} // namespace probe::vr
