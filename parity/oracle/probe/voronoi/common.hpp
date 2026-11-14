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
// What the voronoi probe's sources share (parity/sop1/voronoi.md, "Values"): the committed-set
// budget of a kind, the writers of points, polygons, sites, circle events and whole diagrams, the
// log capture, the seeded generator and the inputs every piece's records start from: the site
// sets (points and segments in 32-bit coordinates, as Boost.Polygon's builder takes them) and the
// shapes (ExPolygons in scaled coordinates: hand-made, the Voronoi issue polygons of OrcaSlicer's
// tests/libslic3r/test_voronoi.cpp, slices of the OBJ meshes of src/tests/data and seeded stars).
// Written by orca-o3-probe-voronoi, 2026-11-13.
#pragma once

#include "../probe.hpp"

#include <cstdint>
#include <exception>
#include <string>
#include <vector>

#include <boost/polygon/voronoi.hpp>

#include "libslic3r/ExPolygon.hpp"
#include "libslic3r/Line.hpp"
#include "libslic3r/Point.hpp"
#include "libslic3r/Polygon.hpp"
#include "libslic3r/Polyline.hpp"

namespace probe::vr {

namespace bpd = boost::polygon::detail;

using CTT  = bpd::voronoi_ctype_traits<bpd::int32>;
using VP   = bpd::voronoi_predicates<CTT>;
using Pt   = bpd::point_2d<bpd::int32>;
using Site = bpd::site_event<bpd::int32>;
using Circ = bpd::circle_event<double>;
using BigI = bpd::extended_int<64>;
using Efpt = bpd::extended_exponent_fpt<double>;
using Rfpt = bpd::robust_fpt<double>;
using Rdif = bpd::robust_dif<Rfpt>;
using BVD  = boost::polygon::voronoi_diagram<double>;

// ---------------------------------------------------------------- the committed set

// How many records of a kind the committed set takes: every hand-made one and the first `cap`
// others, each of at most `max_bytes`.
struct Budget
{
    size_t cap;
    size_t max_bytes;
    size_t taken = 0;
    bool   take(const Payload &p, bool hand)
    {
        if (p.bytes.size() > max_bytes)
            return false;
        if (hand)
            return true;
        if (taken >= cap)
            return false;
        ++taken;
        return true;
    }
};

// Writes one record into the full set, and into the committed set when `committed`.
inline void put_record(Sets &sets, const char (&kind)[5], const Payload &p, bool committed)
{
    sets.full.add(kind, p);
    if (committed)
        sets.committed.add(kind, p);
}

// ---------------------------------------------------------------- the generator

// The seeded generator: probe.hpp's splitmix64 with the ranges the records draw from.
struct Gen
{
    SplitMix s;
    explicit Gen(uint64_t seed) : s(seed) {}
    // An integer in [lo, hi]; hi - lo below 2^63.
    int64_t range(int64_t lo, int64_t hi) { return lo + int64_t(s.below(uint64_t(hi - lo) + 1)); }
    // A double in [0, 1) from the top 53 bits.
    double unit() { return double(s.next() >> 11) * 0x1.0p-53; }
    double real(double lo, double hi) { return lo + (hi - lo) * unit(); }
    // True one time in n.
    bool chance(uint64_t n) { return s.below(n) == 0; }
    // An integer in [0, n).
    uint64_t below(uint64_t n) { return s.below(n); }
    uint64_t next() { return s.next(); }
};

// ---------------------------------------------------------------- values

void put_pt(Payload &p, const Slic3r::Point &q);
void put_pts(Payload &p, const Slic3r::Points &pts);
void put_polys(Payload &p, const Slic3r::Polygons &polys);
void put_expoly(Payload &p, const Slic3r::ExPolygon &e);
void put_expolys(Payload &p, const Slic3r::ExPolygons &es);
void put_lines(Payload &p, const Slic3r::Lines &lines);
// A site event: i32 x0, y0, x1, y1, u64 sorted index, u64 initial index, u8 source category, u8
// inverse.
void put_site(Payload &p, const Site &s);
// A circle event: f64 x, y, lower x, u8 active.
void put_circle(Payload &p, const Circ &c);
// A big integer: i32 count (its sign the number's), then |count| u32 chunks, lowest first.
void put_big(Payload &p, const BigI &v);
// An extended-exponent double as the object holds it: f64 val_, i32 exp_.
void put_efpt(Payload &p, const Efpt &v);
// A robust_fpt: f64 fpv, f64 re.
void put_rfpt(Payload &p, const Rfpt &v);
// A robust_dif of robust_fpt: its positive sum, then its negative sum.
void put_rdif(Payload &p, const Rdif &v);
// An exception a call threw: u8 1 Slic3r::InvalidArgument, 2 Slic3r::OutOfRange, 3
// Slic3r::RuntimeError, 9 any other, then str what().
void put_error(Payload &p, const std::exception &e);
// The text the log capture holds since it was last taken, then empties it.
std::string take_log();
// Starts the log capture (every Boost.Log record, "[<severity>] <message>\n"); Boost.Log's default
// sink stays off until stop_log.
void start_log();
void stop_log();

// A site event as the builder holds one: points (x0, y0) and (x1, y1) (equal for a point), its
// sorted and initial indexes and its source category, inverted (its points swapped) when
// `inverse`. The constructors leave the initial index unset, so every site the probe makes sets it.
inline Site mk_site(int32_t x0, int32_t y0, int32_t x1, int32_t y1, size_t sorted, size_t initial, boost::polygon::SourceCategory category, bool inverse)
{
    Site s(x0, y0, x1, y1);
    s.sorted_index(sorted).initial_index(initial).source_category(category);
    if (inverse)
        s.inverse();
    return s;
}

// A double of random sign, a 52-bit random mantissa and a binary exponent in [emin, emax].
inline double rand_double(Gen &g, int emin, int emax)
{
    const uint64_t m = g.next() & ((uint64_t(1) << 52) - 1);
    const int64_t  e = g.range(emin, emax);
    uint64_t       b = (uint64_t(e + 1023) << 52) | m;
    if (g.chance(2))
        b |= uint64_t(1) << 63;
    return from_bits(b);
}

// The records, piece by piece (parity/sop1/voronoi.md): arith.cpp the arithmetic and the
// predicates of voronoi-1, builder.cpp the builder runs (voronoi-2, and the predicate calls they
// make, voronoi-1), diagram.cpp the diagrams and utilities of voronoi-3, slic3r.cpp the repair, the
// planarity checks and the medial axis of voronoi-4.
void record_arith(Sets &sets);
// False, with `error`, when a builder run with the recording predicates differs from the default
// builder's.
bool record_builder(Sets &sets, std::string &error);
void record_diagram(Sets &sets);
// False, with `error`, when the rotations a VRGD record lists do not rebuild the diagram
// construct_voronoi's repair left.
bool record_slic3r(Sets &sets, std::string &error);

// The index of `x` in `base`'s container, -1 for none.
template<class T> int32_t index_of(const T *x, const T *base) { return x == nullptr ? -1 : int32_t(x - base); }

// A vertex coordinate: its f64 bits, except that every NaN is written as 0x7ff8000000000000. The
// NaN a degenerate diagram holds is whatever the CPU's arithmetic made, so a replay compares a NaN
// coordinate by NaN-ness only.
inline void put_coord(Payload &p, double v) { p.f64(v != v ? from_bits(0x7FF8000000000000ull) : v); }

// A whole diagram, boost::polygon::voronoi_diagram<double> or Slic3r::Geometry::VoronoiDiagram
// through its accessors: u32 cells, each u64 source index, u8 source category, i32 incident edge,
// u32 colour; u32 vertices, each f64 x, f64 y (put_coord), i32 incident edge, u32 colour; u32
// edges, each i32 cell, i32 vertex0, i32 twin, i32 next, i32 prev, u8 flags (1 linear, 2 primary),
// u32 colour. An index is the element's position in its container, -1 for none.
template<class VD> void put_diagram(Payload &p, const VD &vd)
{
    const auto &cells = vd.cells();
    const auto &verts = vd.vertices();
    const auto &edges = vd.edges();
    const auto *c0    = cells.data();
    const auto *v0    = verts.data();
    const auto *e0    = edges.data();
    p.u32(uint32_t(cells.size()));
    for (const auto &c : cells)
        p.u64(uint64_t(c.source_index())).u8(uint8_t(c.source_category())).i32(index_of(c.incident_edge(), e0)).u32(uint32_t(c.color()));
    p.u32(uint32_t(verts.size()));
    for (const auto &v : verts) {
        put_coord(p, v.x());
        put_coord(p, v.y());
        p.i32(index_of(v.incident_edge(), e0)).u32(uint32_t(v.color()));
    }
    p.u32(uint32_t(edges.size()));
    for (const auto &e : edges) {
        p.i32(index_of(e.cell(), c0)).i32(index_of(e.vertex0(), v0)).i32(index_of(e.twin(), e0)).i32(index_of(e.next(), e0)).i32(index_of(e.prev(), e0));
        p.u8(uint8_t((e.is_linear() ? 1 : 0) | (e.is_primary() ? 2 : 0))).u32(uint32_t(e.color()));
    }
}

// ---------------------------------------------------------------- inputs

struct Pt32
{
    int32_t x, y;
};
struct Seg32
{
    Pt32 a, b;
};

// A builder input: points, then segments, inserted in that order (insert_point, insert_segment).
struct SiteSet
{
    std::string        label;
    std::vector<Pt32>  points;
    std::vector<Seg32> segments;
    bool               hand;
};

// The inputs of a site set: u32 points, each i32 x, y; u32 segments, each i32 x1, y1, x2, y2.
void put_siteset(Payload &p, const SiteSet &s);

// Every site set, in order: the hand-made ones, the shapes' contours as segments (those of at
// most 400 segments) and the seeded ones (seed 0x5EED5E7).
const std::vector<SiteSet> &site_sets();

// A shape: ExPolygons in scaled coordinates.
struct Shape
{
    std::string        label;
    Slic3r::ExPolygons ex;
    bool               hand;
};

// Every shape, in order: the hand-made and issue shapes, the mesh slices, the seeded stars (seed
// 0x57A125).
const std::vector<Shape> &shapes();

// False, with `error` naming the path, when a mesh of src/tests/data the shapes slice could not be
// read: the probe then fails rather than record a smaller input set.
bool inputs_ok(std::string &error);

// Segments given as lines, as test_voronoi.cpp gives some of its cases; `hand` when the record with
// the repair on goes to the committed set (within VRGD's budget).
struct LineSet
{
    std::string   label;
    Slic3r::Lines lines;
    bool          hand;
};

// The line sets of issues.cpp, in order: lines0 to lines4 of "Alessandro gapfill 12707" and the
// lines of "Voronoi NaN coordinates 12139".
const std::vector<LineSet> &issue_line_sets();

// The repair search (search.cpp): kSearchCount draws, draw i from Gen(kSearchSeed + i).
constexpr uint64_t kSearchSeed  = 0x5EA4C8u;
constexpr uint32_t kSearchCount = 20000;

// A draw's family (0 a square with vertices off its edges, 1 a star with vertices off its edges, 2
// two vertices 1 to 3 nm apart, 3 a sliver, 4 a slit or a notch, 5 a hole by the contour) and its
// shape: one ExPolygon, or none when its edges intersect.
struct SearchShape
{
    uint8_t            family;
    Slic3r::ExPolygons ex;
};
SearchShape search_shape(uint32_t draw);

// The medial axis inputs: an ExPolygon with min and max widths.
struct ThinCase
{
    std::string       label;
    Slic3r::ExPolygon ex;
    double            min_width;
    double            max_width;
    bool              hand;
};

// Every medial axis input, in order: hand-made thin shapes, then the thin-wall and gap-fill
// regions of the shapes (as PerimeterGenerator and the fill make them), then seeded thin rings.
const std::vector<ThinCase> &thin_cases();

// A shape's polygons (contours and holes) with the consecutive repeated points and the
// collinear points removed, as Arachne's input to SkeletalTrapezoidation has none.
Slic3r::Polygons arachne_polygons(const Slic3r::ExPolygons &ex);

// The number of segments of a shape's polygons.
size_t segment_count(const Slic3r::Polygons &polys);

} // namespace probe::vr
