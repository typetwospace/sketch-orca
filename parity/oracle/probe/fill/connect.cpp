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
// fill.sop1's FLCN and FLEB records, which piece fill-2 replays (parity/sop1/fill.md;
// orca-o3-probe-fill, 2026-11-13): Fill::connect_infill through its ExPolygon and Polygons
// overloads and Fill::chain_or_connect_infill (FillBase.cpp's contour walks and clipping, the
// boundary infill graph and the connection of the lines), on lines clipped by the boundary as a
// pattern makes them, some chained, some with an end drawn in off the boundary; and
// Fill::extended_object_bounding_box. Each record holds its inputs and then the engine's results.
#include "fill.hpp"

#include <algorithm>
#include <cmath>

#include "libslic3r/ClipperUtils.hpp"
#include "libslic3r/ShortestPath.hpp"

namespace probe {
namespace fill {

using namespace Slic3r;

namespace {

// A filler whose extended_object_bounding_box is Fill's own.
class BoxFill : public Fill
{
public:
    Fill *clone() const override { return new BoxFill(*this); }
    bool  is_self_crossing() override { return false; }
};

struct ConnectCase
{
    uint8_t    entry = 0; // 0 connect_infill(ExPolygon), 1 connect_infill(Polygons, bbox), 2 chain_or_connect_infill
    ExPolygon  boundary;
    BoundingBox bbox;
    double     spacing = 0.45;
    FillParams params;
    Polylines  infill;
};

Payload put_connect(const ConnectCase &c)
{
    Payload p;
    p.u8(c.entry);
    put(p, c.boundary);
    put(p, c.bbox);
    p.f64(c.spacing);
    put(p, c.params);
    put(p, c.infill);
    Polylines out;
    try {
        Polylines in = c.infill;
        if (c.entry == 0)
            Fill::connect_infill(std::move(in), c.boundary, out, c.spacing, c.params);
        else if (c.entry == 1)
            Fill::connect_infill(std::move(in), to_polygons(c.boundary), c.bbox, out, c.spacing, c.params);
        else
            Fill::chain_or_connect_infill(std::move(in), c.boundary, out, c.spacing, c.params);
        p.u8(0);
        put(p, out);
    } catch (const std::exception &e) {
        put_threw(p, e);
    }
    return p;
}

// The lines of a case: clipped by the boundary, chained or not, now and then one end drawn in off
// the boundary by `pull` millimetres, or reversed.
Polylines case_lines(Gen &g, const ExPolygon &e, double angle, double line_spacing, bool chain, double pull)
{
    Polylines lines = clipped_lines(e, angle, line_spacing, g.real(0, 1));
    if (chain && lines.size() > 1)
        lines = chain_polylines(std::move(lines));
    for (Polyline &pl : lines) {
        if (pl.points.size() >= 2 && pull > 0 && g.one_in(5)) {
            const double len = pl.length();
            if (len > 3. * double(mm(pull)))
                pl.clip_end(double(mm(pull)));
        }
        if (g.one_in(6))
            pl.reverse();
    }
    return lines;
}

FillParams connect_params(float anchor, float anchor_max, bool dont_sort)
{
    FillParams q;
    q.density           = 0.2f;
    q.anchor_length     = anchor;
    q.anchor_length_max = anchor_max;
    q.dont_sort         = dont_sort;
    q.pattern           = ipCrossHatch;
    return q;
}

} // namespace

void record_connect(Sets &sets, const std::vector<Shape> &sections)
{
    {
        Recorder                 r(sets, "FLCN", 40, 6000, 70000);
        const std::vector<Shape> shapes = hand_shapes();
        Gen                      g(0xF1A0005ull);
        // The anchor settings, entries, chaining and drawn-in ends on three shapes with holes, then
        // every hand shape with anchors as the sparse infill has them by default (400% of the
        // spacing, at most 20 mm).
        const struct
        {
            uint8_t entry;
            float   anchor, anchor_max;
            bool    dont_sort, chain;
            double  pull;
        } variants[] = {{0, 0.f, 0.f, false, false, 0.},     {2, 0.f, 0.f, false, false, 0.},  {2, 0.f, 0.04f, false, true, 0.},
                        {0, 1000.f, 1000.f, false, true, 0.}, {0, 1.8f, 20.f, true, true, 0.},  {1, 1.8f, 20.f, false, true, 0.},
                        {0, 1.8f, 20.f, false, true, 0.6},   {0, 0.f, 3.f, false, false, 0.},  {0, 5.f, 5.f, false, false, 0.},
                        {2, 0.9f, 1.f, false, false, 0.3}};
        for (const auto &v : variants)
            for (size_t shape : {size_t(2), size_t(11), size_t(7)}) {
                ConnectCase c;
                c.entry    = v.entry;
                c.boundary = shapes[shape].e;
                c.bbox     = get_extents(c.boundary.contour);
                c.params   = connect_params(v.anchor, v.anchor_max, v.dont_sort);
                c.infill   = case_lines(g, c.boundary, 1.1, 1.5, v.chain, v.pull);
                if (c.infill.empty())
                    continue;
                r.edge(put_connect(c));
            }
        for (const Shape &sh : shapes) {
            ConnectCase c;
            c.boundary = sh.e;
            c.bbox     = get_extents(sh.e.contour);
            c.params   = connect_params(1.8f, 20.f, false);
            c.infill   = case_lines(g, sh.e, 0.3, 2.25, false, 0.);
            if (c.infill.empty())
                continue;
            r.edge(put_connect(c));
        }
        for (int i = 0; i < 400; ++i) {
            ConnectCase c;
            c.entry    = uint8_t(g.below(3));
            c.boundary = (!sections.empty() && g.one_in(6)) ? g.pick(sections).e : random_shape(g, g.real(2, 40));
            c.bbox     = get_extents(c.boundary.contour);
            if (c.entry == 1 && g.one_in(3))
                c.bbox.offset(mm(g.real(0, 3)));
            c.spacing = g.real(0.3, 0.8);
            const float anchor_max = g.one_in(5) ? float(g.real(0, 0.06)) : g.one_in(4) ? 1000.f : float(g.real(0.1, 30));
            const float anchor     = g.one_in(4) ? 0.f : std::min(anchor_max, float(g.real(0, 10)));
            c.params   = connect_params(anchor, anchor_max, g.one_in(6));
            c.infill   = case_lines(g, c.boundary, g.real(-PI, PI), c.spacing / g.real(0.08, 1.), g.one_in(2), g.one_in(4) ? g.real(0.1, 1.) : 0.);
            if (c.infill.empty() && c.entry != 2)
                continue;
            r.seeded(put_connect(c));
        }
    }
    {
        Recorder r(sets, "FLEB", 100, 128);
        auto     one = [](const BoundingBox &b) {
            BoxFill f;
            f.bounding_box = b;
            Payload p;
            put(p, b);
            put(p, f.extended_object_bounding_box());
            return p;
        };
        r.edge(one(BoundingBox(Point(0, 0), Point(-1, -1))));
        r.edge(one(BoundingBox()));
        r.edge(one(BoundingBox(Point(mm(-10), mm(-10)), Point(mm(10), mm(10)))));
        r.edge(one(BoundingBox(Point(mm(5), mm(-40)), Point(mm(120), mm(3)))));
        r.edge(one(BoundingBox(Point(-1000000000, 7), Point(1000000000, 9))));
        Gen g(0xF1A0006ull);
        for (int i = 0; i < 100; ++i) {
            const int64_t x = mm(g.real(-200, 200)), y = mm(g.real(-200, 200));
            r.seeded(one(BoundingBox(Point(x, y), Point(x + mm(g.real(0, 250)), y + mm(g.real(0, 250))))));
        }
    }
}

} // namespace fill
} // namespace probe
