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
// index.sop1's EdgeGrid records (parity/sop1/index.md), which orca-index replays for the piece
// index-2:
//
//   IXEG  EdgeGrid::Grid made by one of its create() overloads (Polygons, pointers to Polygons,
//         lists of Points open or closed, Polygons with Polylines, an ExPolygon, ExPolygons),
//         with or without a box set first: the grid's box, resolution, rows and columns, its
//         contours (where each starts in the input, its points, whether it is open, its segments,
//         get_segments() and each segment's segment_idx_prev, segment_idx_next and
//         segment_prev), its cells and their contour and segment pairs; then
//         visit_cells_intersecting_line and visit_cells_intersecting_box (with a visitor that
//         stops after a given number of cells), closest_point_signed_distance,
//         signed_distance_edges and signed_distance at points, contours_simplified,
//         intersecting_edges and has_intersecting_edges; then calculate_sdf's field (open
//         contours included: the release build reads an open contour's last point before its
//         first), signed_distance and signed_distance_bilinear again at the
//         points, and contours_simplified again (it reads the field).
//   IXEI  The free intersecting_edges(Polygons) (TreeSupport3D.cpp's check): whether it found
//         any, and each pair's two segment indices. The pairs' contours are pointers into the
//         function's own grid, gone once it returns, so they are not recorded.
//
// The grids' cells and field are protected members; GridProbe, a subclass of EdgeGrid::Grid,
// reads them. The inputs: squares either way round, squares with holes, two
// ExPolygons, a circle, a slice-sized ExPolygon, edges through cell corners in all four
// directions, a spike, a bow tie, collinear points, a repeated closing point, a single point, no
// contour, open polylines, boxes set before create (one undefined), large coordinates, coarse
// companions of five of them (so that the committed set holds every create overload and the bow
// tie) and seeded shapes. Each record holds its inputs and then the engine's results. Recorded by
// orca-o3-probe-index, 2026-11-13.
#include "index.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <functional>
#include <string>
#include <utility>
#include <vector>

#include "libslic3r/BoundingBox.hpp"
#include "libslic3r/EdgeGrid.hpp"
#include "libslic3r/ExPolygon.hpp"
#include "libslic3r/Polygon.hpp"
#include "libslic3r/Polyline.hpp"

namespace probe {

namespace {

using namespace Slic3r;
using namespace probe::ix;

// Reads what EdgeGrid::Grid keeps protected: the cells, their contour and segment pairs and the
// signed distance field.
struct GridProbe : public EdgeGrid::Grid
{
    size_t                                        cell_count() const { return m_cells.size(); }
    size_t                                        cell_begin(size_t i) const { return m_cells[i].begin; }
    size_t                                        cell_end(size_t i) const { return m_cells[i].end; }
    const std::vector<std::pair<size_t, size_t>> &cell_data() const { return m_cell_data; }
    const std::vector<float>                     &field() const { return m_signed_distance_field; }
};

// One input path: its role (0 a Polygon, 1 a Polyline, 2 an ExPolygon's contour, 3 a hole, 4 a
// list of Points), the ExPolygon it belongs to, and its points.
struct Path
{
    uint8_t  role;
    uint32_t group;
    Points   pts;
};

struct Visit
{
    Point    a, b;
    uint64_t stop;
};
struct BoxVisit
{
    BoundingBox box;
    uint64_t    stop;
};
struct At
{
    Point   pt;
    coord_t radius;
};

struct GridCase
{
    std::string                         label;
    uint8_t                             overload; // 0 Polygons, 1 pointers, 2 Points lists, 3 Polygons and Polylines, 4 ExPolygon, 5 ExPolygons
    bool                                open = false;
    std::vector<Path>                   paths;
    bool                                has_bbox = false;
    BoundingBox                         bbox;
    coord_t                             resolution = 100;
    std::vector<Visit>                  lines;
    std::vector<BoxVisit>               boxes;
    std::vector<At>                     points;
    std::vector<std::pair<coord_t, bool>> simplify;
    bool                                sdf = true;
};

struct Collect
{
    std::vector<std::pair<coord_t, coord_t>> cells;
    uint64_t                                 stop;
    bool                                     operator()(coord_t iy, coord_t ix)
    {
        cells.emplace_back(iy, ix);
        return stop == 0 || cells.size() < stop;
    }
};

void put_cells(Payload &p, const std::vector<std::pair<coord_t, coord_t>> &cells)
{
    p.u64(cells.size());
    for (const auto &c : cells)
        p.i64(c.first).i64(c.second);
}

void put_polygons(Payload &p, const Polygons &polys)
{
    p.u64(polys.size());
    for (const Polygon &poly : polys)
        put(p, poly.points);
}

void put_bbox(Payload &p, const BoundingBox &b)
{
    put(p, b.min);
    put(p, b.max);
    p.boolean(b.defined);
}

void ixeg(Payload &p, const GridCase &c)
{
    p.str(c.label);
    p.u8(c.overload);
    p.boolean(c.open);
    p.u64(c.paths.size());
    for (const Path &path : c.paths) {
        p.u8(path.role).u32(path.group);
        put(p, path.pts);
    }
    p.boolean(c.has_bbox);
    if (c.has_bbox)
        put_bbox(p, c.bbox);
    p.i64(c.resolution);

    // The containers create() is given; the grid's contours point into them, so they live as
    // long as the grid. `sources` holds each path's points as the grid sees them.
    Polygons                   polygons;
    Polylines                  polylines;
    std::vector<Points>        lists;
    ExPolygons                 expolys;
    std::vector<const Points *> sources(c.paths.size(), nullptr);
    for (const Path &path : c.paths) {
        if (path.role == 0)
            polygons.emplace_back(path.pts);
        else if (path.role == 1)
            polylines.emplace_back(path.pts);
        else if (path.role == 4)
            lists.push_back(path.pts);
        else {
            if (expolys.size() <= path.group)
                expolys.resize(path.group + 1);
            if (path.role == 2)
                expolys[path.group].contour = Polygon(path.pts);
            else
                expolys[path.group].holes.emplace_back(path.pts);
        }
    }
    {
        size_t                             ipoly = 0, iline = 0, ilist = 0;
        std::vector<size_t>                holes(expolys.size(), 0);
        for (size_t i = 0; i < c.paths.size(); ++i) {
            const Path &path = c.paths[i];
            if (path.role == 0)
                sources[i] = &polygons[ipoly++].points;
            else if (path.role == 1)
                sources[i] = &polylines[iline++].points;
            else if (path.role == 4)
                sources[i] = &lists[ilist++];
            else if (path.role == 2)
                sources[i] = &expolys[path.group].contour.points;
            else
                sources[i] = &expolys[path.group].holes[holes[path.group]++].points;
        }
    }
    std::vector<const Polygon *> pointers;
    for (const Polygon &poly : polygons)
        pointers.push_back(&poly);

    // Overload 4 takes the first ExPolygon (by reference, so the contours point into `expolys`).
    const ExPolygon none;
    GridProbe       grid;
    if (c.has_bbox)
        grid.set_bbox(c.bbox);
    switch (c.overload) {
    case 0: grid.create(polygons, c.resolution); break;
    case 1: grid.create(pointers, c.resolution); break;
    case 2: grid.create(lists, c.resolution, c.open); break;
    case 3: grid.create(polygons, polylines, c.resolution); break;
    case 4:
        if (expolys.empty())
            grid.create(none, c.resolution);
        else
            grid.create(expolys.front(), c.resolution);
        break;
    default: grid.create(expolys, c.resolution); break;
    }

    put_bbox(p, grid.bbox());
    p.i64(grid.resolution()).u64(grid.rows()).u64(grid.cols());

    const std::vector<EdgeGrid::Contour> &contours = grid.contours();
    p.u64(contours.size());
    for (const EdgeGrid::Contour &ct : contours) {
        uint64_t from = NPOS, offset = 0;
        for (size_t i = 0; i < sources.size(); ++i) {
            const Points *s = sources[i];
            if (s != nullptr && !s->empty() && ct.begin() >= s->data() && ct.begin() < s->data() + s->size()) {
                from   = i;
                offset = uint64_t(ct.begin() - s->data());
            }
        }
        p.u64(from).u64(offset).u64(uint64_t(ct.end() - ct.begin()));
        put(p, Points(ct.begin(), ct.end()));
        p.boolean(ct.open()).u64(ct.num_segments());
        const Lines segs = ct.get_segments();
        p.u64(segs.size());
        for (const Line &l : segs) {
            put(p, l.a);
            put(p, l.b);
        }
        // Each segment's neighbours, as ElephantFootCompensation.cpp reads them. Every contour
        // create() keeps has a segment: a closed one has a point, an open one two. Segment 0 of an
        // open contour is past the asserts; the release build gives the last point and the last
        // index there.
        for (size_t i = 0; i < ct.num_segments(); ++i) {
            p.u64(ct.segment_idx_prev(i)).u64(ct.segment_idx_next(i));
            put(p, ct.segment_prev(i));
        }
    }
    p.u64(grid.cell_count());
    for (size_t i = 0; i < grid.cell_count(); ++i)
        p.u64(grid.cell_begin(i)).u64(grid.cell_end(i));
    p.u64(grid.cell_data().size());
    for (const auto &cd : grid.cell_data())
        p.u64(cd.first).u64(cd.second);

    p.u64(c.lines.size());
    for (const Visit &v : c.lines) {
        put(p, v.a);
        put(p, v.b);
        p.u64(v.stop);
        Collect col;
        col.stop = v.stop;
        grid.visit_cells_intersecting_line(v.a, v.b, col);
        put_cells(p, col.cells);
    }
    p.u64(c.boxes.size());
    for (const BoxVisit &v : c.boxes) {
        put(p, v.box.min);
        put(p, v.box.max);
        p.u64(v.stop);
        Collect col;
        col.stop = v.stop;
        grid.visit_cells_intersecting_box(v.box, col);
        put_cells(p, col.cells);
    }
    p.u64(c.points.size());
    for (const At &a : c.points) {
        put(p, a.pt);
        p.i64(a.radius);
        const EdgeGrid::Grid::ClosestPointResult r = grid.closest_point_signed_distance(a.pt, a.radius);
        p.u64(r.contour_idx).u64(r.start_point_idx).f64(r.distance).f64(r.t);
        coordf_t d  = 12345.5;
        bool     on = false;
        p.boolean(grid.signed_distance_edges(a.pt, a.radius, d, &on));
        p.f64(d).boolean(on);
        coordf_t d2 = 12345.5;
        p.boolean(grid.signed_distance(a.pt, a.radius, d2));
        p.f64(d2);
    }
    p.u64(c.simplify.size());
    for (const auto &s : c.simplify) {
        p.i64(s.first).boolean(s.second);
        put_polygons(p, grid.contours_simplified(s.first, s.second));
    }
    const auto ie = grid.intersecting_edges();
    p.u64(ie.size());
    for (const auto &pr : ie)
        p.u64(uint64_t(pr.first.first - contours.data())).u64(pr.first.second).u64(uint64_t(pr.second.first - contours.data())).u64(pr.second.second);
    p.boolean(grid.has_intersecting_edges());

    p.boolean(c.sdf);
    if (!c.sdf)
        return;
    grid.calculate_sdf();
    p.u64(grid.field().size());
    for (float f : grid.field())
        p.f32(f);
    for (const At &a : c.points) {
        coordf_t d = 12345.5;
        p.boolean(grid.signed_distance(a.pt, a.radius, d));
        p.f64(d);
        p.f32(grid.signed_distance_bilinear(a.pt));
    }
    for (const auto &s : c.simplify)
        put_polygons(p, grid.contours_simplified(s.first, s.second));
}

Points square(coord_t x0, coord_t y0, coord_t x1, coord_t y1, bool ccw = true)
{
    Points pts{Point(x0, y0), Point(x1, y0), Point(x1, y1), Point(x0, y1)};
    if (!ccw)
        std::reverse(pts.begin(), pts.end());
    return pts;
}

Points circle(coord_t cx, coord_t cy, coord_t r, size_t n, bool ccw = true)
{
    Points pts;
    for (size_t i = 0; i < n; ++i) {
        const double a = 2. * 3.141592653589793 * double(i) / double(n);
        pts.emplace_back(cx + coord_t(std::floor(double(r) * std::cos(a) + 0.5)), cy + coord_t(std::floor(double(r) * std::sin(a) + 0.5)));
    }
    if (!ccw)
        std::reverse(pts.begin(), pts.end());
    return pts;
}

Points star(SplitMix &g, coord_t cx, coord_t cy, coord_t r, size_t n, bool ccw = true)
{
    Points pts;
    for (size_t i = 0; i < n; ++i) {
        const double a = 2. * 3.141592653589793 * double(i) / double(n);
        const double k = real(g, 0.35, 1.0);
        pts.emplace_back(cx + coord_t(std::floor(double(r) * k * std::cos(a))), cy + coord_t(std::floor(double(r) * k * std::sin(a))));
    }
    if (!ccw)
        std::reverse(pts.begin(), pts.end());
    return pts;
}

// The queries of a grid case, from its points and the box create() will make (the points' box,
// widened by 16, merged with a defined box set first): lines inside the cells (from vertices,
// seeded points and cell corners, level, upright and one a point), boxes (inside, across the edge,
// inverted, everything), points (vertices, midpoints, seeded, outside and far off) at radii from
// 0 to two cells, and contours_simplified's offsets.
void add_queries(SplitMix &g, GridCase &c, size_t nlines, size_t nboxes, size_t npoints)
{
    BoundingBox b = c.has_bbox && c.bbox.defined ? c.bbox : BoundingBox();
    Points      all;
    for (const Path &path : c.paths)
        if (!((c.overload == 2 || c.overload == 3) && path.pts.size() <= 1) && !(c.overload == 4 && path.group != 0))
            for (const Point &q : path.pts) {
                b.merge(q);
                all.push_back(q);
            }
    b.min -= Point(16, 16);
    b.max += Point(16, 16);
    const coord_t res  = c.resolution;
    const coord_t cols = (b.max.x() - b.min.x() + res - 1) / res, rows = (b.max.y() - b.min.y() + res - 1) / res;
    const coord_t xmax = std::min(b.max.x(), b.min.x() + cols * res - 1), ymax = std::min(b.max.y(), b.min.y() + rows * res - 1);
    auto          inside = [&]() -> Point {
        const int64_t k = all.empty() ? 1 : rnd(g, 0, 3);
        if (k == 0)
            return all[size_t(g.below(all.size()))];
        if (k == 1)
            return Point(rnd(g, b.min.x(), xmax), rnd(g, b.min.y(), ymax));
        if (k == 2)
            return Point(std::min(xmax, b.min.x() + rnd(g, 0, cols) * res), std::min(ymax, b.min.y() + rnd(g, 0, rows) * res));
        const Point q = all[size_t(g.below(all.size()))];
        return Point(std::clamp<coord_t>(q.x() + rnd(g, -res, res), b.min.x(), xmax), std::clamp<coord_t>(q.y() + rnd(g, -res, res), b.min.y(), ymax));
    };
    for (size_t i = 0; i < nlines; ++i) {
        Point a = inside(), e = inside();
        if (i % 6 == 4)
            e = Point(e.x(), a.y());
        else if (i % 6 == 5)
            e = Point(a.x(), e.y());
        else if (i == 3)
            e = a;
        c.lines.push_back(Visit{a, e, uint64_t(i % 5 == 2 ? 1 : (i % 5 == 3 ? 3 : 0))});
    }
    for (size_t i = 0; i < nboxes; ++i) {
        Point a(rnd(g, b.min.x() - 2 * res, b.max.x() + 2 * res), rnd(g, b.min.y() - 2 * res, b.max.y() + 2 * res));
        Point e(rnd(g, b.min.x() - 2 * res, b.max.x() + 2 * res), rnd(g, b.min.y() - 2 * res, b.max.y() + 2 * res));
        BoundingBox box;
        if (i == 0)
            box = BoundingBox(b.min - Point(res, res), b.max + Point(res, res));
        else if (i % 4 == 3)
            box = BoundingBox(Point(std::max(a.x(), e.x()), std::max(a.y(), e.y())), Point(std::min(a.x(), e.x()), std::min(a.y(), e.y())));
        else
            box = BoundingBox(Point(std::min(a.x(), e.x()), std::min(a.y(), e.y())), Point(std::max(a.x(), e.x()), std::max(a.y(), e.y())));
        c.boxes.push_back(BoxVisit{box, uint64_t(i % 3 == 2 ? 2 : 0)});
    }
    const coord_t radii[] = {0, res / 2, res, 2 * res, 100};
    for (size_t i = 0; i < npoints; ++i) {
        Point q;
        const int64_t k = all.empty() ? 3 : rnd(g, 0, 4);
        if (k == 0)
            q = all[size_t(g.below(all.size()))];
        else if (k == 1 && all.size() > 1) {
            const size_t j = size_t(g.below(all.size() - 1));
            q              = Point((all[j].x() + all[j + 1].x()) / 2, (all[j].y() + all[j + 1].y()) / 2);
        } else if (k == 2)
            q = Point(rnd(g, b.min.x() - 3 * res, b.max.x() + 3 * res), rnd(g, b.min.y() - 3 * res, b.max.y() + 3 * res));
        else
            q = inside();
        c.points.push_back(At{q, radii[size_t(g.below(5))]});
    }
    c.points.push_back(At{Point(b.max.x() + 7 * res, b.min.y() - 5 * res), 2 * res});
    c.points.push_back(At{Point(b.min.x() - 3 * res, b.max.y() + 9 * res), res});
    c.simplify = {{0, false}, {0, true}, {res / 4, false}, {-(res / 4), true}};
}

// A case with add_queries' 10 lines, 5 boxes and 12 points; FEW instead gives a case fewer
// queries, so that it fits the committed set's limit for IXEG.
struct Queries
{
    size_t lines, boxes, points;
};
constexpr Queries MANY{10, 5, 12}, FEW{4, 2, 5};

GridCase make(std::string label, uint8_t overload, std::vector<Path> paths, coord_t res, bool sdf, uint64_t seed, bool open = false, Queries n = MANY)
{
    GridCase c;
    c.label      = std::move(label);
    c.overload   = overload;
    c.open       = open;
    c.paths      = std::move(paths);
    c.resolution = res;
    c.sdf        = sdf;
    SplitMix g(seed);
    add_queries(g, c, n.lines, n.boxes, n.points);
    return c;
}

Path poly(Points pts) { return Path{0, 0, std::move(pts)}; }
Path pline(Points pts) { return Path{1, 0, std::move(pts)}; }
Path contour(uint32_t group, Points pts) { return Path{2, group, std::move(pts)}; }
Path hole(uint32_t group, Points pts) { return Path{3, group, std::move(pts)}; }
Path list(Points pts) { return Path{4, 0, std::move(pts)}; }

void record_ixeg(Sets &sets)
{
    using Fn = std::function<void(Payload &)>;
    std::vector<Fn> e;
    auto            add = [&](GridCase c) { e.push_back([c](Payload &p) { ixeg(p, c); }); };
    uint64_t        seed = 0x1B0F0001u;
    // The paths that a coarse companion at the end uses again.
    Points closed = square(1200, 0, 1600, 400);
    closed.push_back(closed.front());
    const std::vector<Path> two_expolygons = {contour(0, square(0, 0, 1000, 1000)), hole(0, square(300, 300, 700, 700, false)), contour(1, square(1500, -200, 2600, 900)),
                                              hole(1, square(1700, 0, 1900, 200, false)), hole(1, square(2100, 400, 2400, 700, false))};
    const std::vector<Path> pointers       = {poly(circle(0, 0, 600, 24)), poly(square(900, -300, 1500, 300))};
    const std::vector<Path> open_lists = {list({Point(0, 0), Point(500, 200), Point(900, 900), Point(100, 800)}), list(closed), list({Point(-300, 1000), Point(400, 1300)}),
                                          list({Point(5, 5)})};
    const std::vector<Path> polys_and_lines = {poly(square(0, 0, 1000, 1000)), pline({Point(-200, -200), Point(300, 1200), Point(800, -100)}), pline(closed),
                                               pline({Point(7, 7)}), poly({Point(3, 3)})};
    const std::vector<Path> bow_tie = {poly({Point(0, 0), Point(1000, 1000), Point(1000, 0), Point(0, 1000)}), poly(square(800, 800, 1800, 1800)),
                                       poly(square(1200, 1200, 1500, 1500))};
    add(make("square", 0, {poly(square(0, 0, 1000, 1000))}, 100, true, seed++));
    add(make("square cw", 0, {poly(square(0, 0, 1000, 1000, false))}, 100, true, seed++));
    add(make("square with hole", 4, {contour(0, square(0, 0, 2000, 2000)), hole(0, square(500, 500, 1500, 1500, false))}, 150, true, seed++));
    add(make("two expolygons", 5, two_expolygons, 130, true, seed++));
    add(make("pointers", 1, pointers, 120, true, seed++));
    {
        Points sq = square(0, 0, 1000, 1000);
        sq.push_back(sq.front());
        add(make("repeated closing point", 2, {list(sq), list(square(1200, 0, 1500, 300))}, 100, true, seed++));
    }
    add(make("open lists", 2, open_lists, 100, true, seed++, true));
    add(make("polygons and polylines", 3, polys_and_lines, 100, true, seed++));
    add(make("no contour", 0, {poly({})}, 10, true, seed++));
    add(make("one point", 0, {poly({Point(40, 70)})}, 50, true, seed++));
    {
        // FEW queries, so that the committed set holds a defined box set before create.
        GridCase c;
        c.label      = "box set first";
        c.overload   = 0;
        c.paths      = {poly(square(0, 0, 1000, 1000))};
        c.has_bbox   = true;
        c.bbox       = BoundingBox(Point(-5000, -3000), Point(4000, 5000));
        c.resolution = 500;
        SplitMix g(seed++);
        add_queries(g, c, FEW.lines, FEW.boxes, FEW.points);
        add(c);
    }
    {
        GridCase c;
        c.label      = "undefined box set first";
        c.overload   = 0;
        c.paths      = {poly(square(0, 0, 1000, 1000))};
        c.has_bbox   = true;
        c.bbox       = BoundingBox(Point(10, 10), Point(5, 5));
        c.resolution = 100;
        SplitMix g(seed++);
        add_queries(g, c, 10, 5, 12);
        add(c);
    }
    add(make("corners", 0,
             {poly({Point(500, 0), Point(1000, 500), Point(500, 1000), Point(0, 500)}), poly({Point(0, 0), Point(1000, 1000), Point(0, 1000)}),
              poly({Point(1100, 1000), Point(2100, 0), Point(2100, 1000)})},
             100, true, seed++));
    add(make("spike", 0, {poly({Point(0, 0), Point(1000, 0), Point(1000, 1000), Point(500, 1000), Point(500, 1600), Point(500, 1000), Point(0, 1000)})}, 100, true, seed++));
    add(make("bow tie", 0, bow_tie, 120, true, seed++));
    add(make("collinear", 0, {poly({Point(0, 0), Point(250, 0), Point(500, 0), Point(1000, 0), Point(1000, 500), Point(1000, 1000), Point(0, 1000), Point(0, 600)})}, 100, true,
             seed++));
    add(make("circle", 0, {poly(circle(0, 0, 5000, 64))}, 1000, true, seed++));
    add(make("slice", 4, {contour(0, square(-10000000, -10000000, 10000000, 10000000)), hole(0, circle(0, 0, 2500000, 48, false))}, 1000000, true, seed++));
    add(make("large", 0, {poly(square(-120000000, -120000000, 120000000, 120000000)), poly(circle(30000000, 0, 40000000, 32, false))}, 10000000, true, seed++));
    // Coarse companions: the same paths on a grid four times coarser, with FEW queries. Their
    // originals pass the committed limit even with FEW queries (the grid, its field and the
    // simplified contours are most of a record), so these are how the committed set holds the
    // create overloads 1, 2 with open, 3 and 5 and the bow tie (a contour that crosses itself and
    // two that cross each other, so intersecting_edges finds pairs).
    add(make("two expolygons, coarse", 5, two_expolygons, 520, true, seed++, false, FEW));
    add(make("pointers, coarse", 1, pointers, 480, true, seed++, false, FEW));
    add(make("open lists, coarse", 2, open_lists, 400, true, seed++, true, FEW));
    add(make("polygons and polylines, coarse", 3, polys_and_lines, 400, true, seed++, false, FEW));
    add(make("bow tie, coarse", 0, bow_tie, 480, true, seed++, false, FEW));

    SplitMix        g(0x1B0F1000u);
    std::vector<Fn> random;
    for (int i = 0; i < 240; ++i) {
        const coord_t     r        = coord_t(1) << rnd(g, 8, 22);
        const coord_t     res      = std::max<coord_t>(1, r / rnd(g, 2, 12));
        const uint8_t     overload = uint8_t(g.below(6));
        const coord_t     cx = rnd(g, -r, r), cy = rnd(g, -r, r);
        std::vector<Path> paths;
        const size_t      n   = size_t(rnd(g, 3, 28));
        bool              open = false;
        switch (overload) {
        case 0:
        case 1:
            paths.push_back(poly(star(g, cx, cy, r, n)));
            if (rnd(g, 0, 1) == 0)
                paths.push_back(poly(star(g, cx + r, cy, r / 2, size_t(rnd(g, 3, 12)), rnd(g, 0, 1) == 0)));
            break;
        case 2:
            open = rnd(g, 0, 1) == 0;
            paths.push_back(list(star(g, cx, cy, r, n)));
            paths.push_back(list(star(g, cx, cy, r / 4, size_t(rnd(g, 3, 10)), false)));
            break;
        case 3:
            paths.push_back(poly(star(g, cx, cy, r, n)));
            paths.push_back(pline({Point(cx - r, cy - r), Point(cx + r / 2, cy + r / 3), Point(cx + r, cy - r / 2)}));
            break;
        case 4:
            paths.push_back(contour(0, star(g, cx, cy, r, n)));
            paths.push_back(hole(0, star(g, cx, cy, r / 4, size_t(rnd(g, 3, 10)), false)));
            break;
        default:
            paths.push_back(contour(0, star(g, cx, cy, r, n)));
            paths.push_back(hole(0, star(g, cx, cy, r / 5, size_t(rnd(g, 3, 8)), false)));
            paths.push_back(contour(1, star(g, cx + 3 * r, cy, r / 2, size_t(rnd(g, 3, 12)))));
            break;
        }
        GridCase c = make("seeded", overload, paths, res, true, g.next(), open);
        random.push_back([c](Payload &p) { ixeg(p, c); });
    }
    size_t next = 0;
    record_cases(sets, "IXEG", e, random.size(), 2, 16384, [&](Payload &p) { random[next++](p); });
}

void ixei(Payload &p, const Polygons &polys)
{
    p.u64(polys.size());
    for (const Polygon &poly : polys)
        put(p, poly.points);
    const auto out = intersecting_edges(polys);
    p.boolean(out.empty());
    p.u64(out.size());
    for (const auto &pr : out)
        p.u64(pr.first.second).u64(pr.second.second);
}

void record_ixei(Sets &sets)
{
    using Fn = std::function<void(Payload &)>;
    std::vector<Fn> e;
    auto            add = [&](Polygons polys) { e.push_back([polys](Payload &p) { ixei(p, polys); }); };
    add({});
    add({Polygon({Point(0, 0), Point(1000, 1000), Point(1000, 0), Point(0, 1000)})});
    add({Polygon(square(0, 0, 1000, 1000)), Polygon(square(500, 500, 1500, 1500))});
    add({Polygon(square(0, 0, 1000, 1000)), Polygon(square(200, 200, 800, 800, false))});
    add({Polygon(square(0, 0, 1000, 1000)), Polygon(square(1000, 0, 2000, 1000))});
    add({Polygon(square(0, 0, 1000, 1000)), Polygon({Point(5000, 5000)}), Polygon({Point(-300, 500), Point(1300, 500)})});
    add({Polygon(circle(0, 0, 3000, 40)), Polygon(circle(2000, 0, 3000, 40)), Polygon(circle(1000, 2500, 800, 12, false))});

    SplitMix        g(0x1B0F2000u);
    std::vector<Fn> random;
    for (int i = 0; i < 200; ++i) {
        const coord_t r = coord_t(1) << rnd(g, 10, 24);
        Polygons      polys;
        const size_t  n = size_t(rnd(g, 1, 4));
        for (size_t j = 0; j < n; ++j) {
            Points pts;
            const size_t m = size_t(rnd(g, 3, 14));
            for (size_t k = 0; k < m; ++k)
                pts.emplace_back(rnd(g, -r, r), rnd(g, -r, r));
            polys.emplace_back(pts);
        }
        random.push_back([polys](Payload &p) { ixei(p, polys); });
    }
    size_t next = 0;
    record_cases(sets, "IXEI", e, random.size(), 16, 1536, [&](Payload &p) { random[next++](p); });
}

} // namespace

void record_index_edgegrid(Sets &sets)
{
    record_ixeg(sets);
    record_ixei(sets);
}

} // namespace probe
