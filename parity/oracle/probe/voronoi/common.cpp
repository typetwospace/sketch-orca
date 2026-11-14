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
// The voronoi probe's shared writers, log capture and inputs (common.hpp; parity/sop1/voronoi.md,
// "Inputs"). Written by orca-o3-probe-voronoi, 2026-11-13.
//
// Carries OrcaSlicer code: the coordinates of the issue polygons come from OrcaSlicer v2.4.2's
// tests/libslic3r/test_voronoi.cpp ("Voronoi missing vertex 1", "2" and "3", "Duplicate Voronoi
// vertices", "Intersecting Voronoi edges", "Voronoi missing edges - points 12067" and "Alessandro
// gapfill 12707", "Voronoi weirdness" and "Voronoi division by zero 12903"), taken 2026-11-13.
#include "common.hpp"

#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <memory>
#include <sstream>

#include <boost/core/null_deleter.hpp>
#include <boost/log/core.hpp>
#include <boost/log/expressions.hpp>
#include <boost/log/sinks/sync_frontend.hpp>
#include <boost/log/sinks/text_ostream_backend.hpp>
#include <boost/log/trivial.hpp>
#include <boost/make_shared.hpp>
#include <boost/shared_ptr.hpp>

#include <oneapi/tbb/global_control.h>

#include "libslic3r/ClipperUtils.hpp"
#include "libslic3r/Exception.hpp"
#include "libslic3r/TriangleMesh.hpp"
#include "libslic3r/TriangleMeshSlicer.hpp"

namespace probe::vr {

using namespace Slic3r;

// ---------------------------------------------------------------- values

void put_pt(Payload &p, const Point &q) { p.i64(q.x()).i64(q.y()); }

void put_pts(Payload &p, const Points &pts)
{
    p.u32(uint32_t(pts.size()));
    for (const Point &q : pts)
        put_pt(p, q);
}

void put_polys(Payload &p, const Polygons &polys)
{
    p.u32(uint32_t(polys.size()));
    for (const Polygon &q : polys)
        put_pts(p, q.points);
}

void put_expoly(Payload &p, const ExPolygon &e)
{
    put_pts(p, e.contour.points);
    put_polys(p, e.holes);
}

void put_expolys(Payload &p, const ExPolygons &es)
{
    p.u32(uint32_t(es.size()));
    for (const ExPolygon &e : es)
        put_expoly(p, e);
}

void put_lines(Payload &p, const Lines &lines)
{
    p.u32(uint32_t(lines.size()));
    for (const Line &l : lines) {
        put_pt(p, l.a);
        put_pt(p, l.b);
    }
}

void put_site(Payload &p, const Site &s)
{
    p.i32(s.x0()).i32(s.y0()).i32(s.x1()).i32(s.y1()).u64(uint64_t(s.sorted_index())).u64(uint64_t(s.initial_index()));
    p.u8(uint8_t(s.source_category())).boolean(s.is_inverse());
}

void put_circle(Payload &p, const Circ &c) { p.f64(c.x()).f64(c.y()).f64(c.lower_x()).boolean(c.is_active()); }

void put_big(Payload &p, const BigI &v)
{
    p.i32(v.count());
    for (size_t i = 0; i < v.size(); ++i)
        p.u32(v.chunks()[i]);
}

void put_efpt(Payload &p, const Efpt &v)
{
    static_assert(sizeof(Efpt) == 16, "extended_exponent_fpt<double> is a double and an int");
    double  val;
    int32_t exp;
    std::memcpy(&val, reinterpret_cast<const char *>(&v), 8);
    std::memcpy(&exp, reinterpret_cast<const char *>(&v) + 8, 4);
    p.f64(val).i32(exp);
}

void put_rfpt(Payload &p, const Rfpt &v) { p.f64(v.fpv()).f64(v.re()); }

void put_rdif(Payload &p, const Rdif &v)
{
    put_rfpt(p, v.pos());
    put_rfpt(p, v.neg());
}

void put_error(Payload &p, const std::exception &e)
{
    uint8_t tag = 9;
    if (dynamic_cast<const Slic3r::InvalidArgument *>(&e))
        tag = 1;
    else if (dynamic_cast<const Slic3r::OutOfRange *>(&e))
        tag = 2;
    else if (dynamic_cast<const Slic3r::RuntimeError *>(&e))
        tag = 3;
    p.u8(tag).str(e.what());
}

void put_siteset(Payload &p, const SiteSet &s)
{
    p.u32(uint32_t(s.points.size()));
    for (const Pt32 &q : s.points)
        p.i32(q.x).i32(q.y);
    p.u32(uint32_t(s.segments.size()));
    for (const Seg32 &g : s.segments)
        p.i32(g.a.x).i32(g.a.y).i32(g.b.x).i32(g.b.y);
}

// ---------------------------------------------------------------- the log capture

namespace {

struct LogCapture
{
    std::ostringstream                                                                               text;
    boost::shared_ptr<boost::log::sinks::synchronous_sink<boost::log::sinks::text_ostream_backend>> sink;
};

LogCapture &capture()
{
    static LogCapture c;
    return c;
}

} // namespace

void start_log()
{
    namespace logging = boost::log;
    namespace sinks   = boost::log::sinks;
    namespace expr    = boost::log::expressions;
    LogCapture &c     = capture();
    if (c.sink)
        return;
    auto backend = boost::make_shared<sinks::text_ostream_backend>();
    backend->add_stream(boost::shared_ptr<std::ostream>(&c.text, boost::null_deleter()));
    backend->auto_flush(true);
    c.sink = boost::make_shared<sinks::synchronous_sink<sinks::text_ostream_backend>>(backend);
    c.sink->set_formatter(expr::stream << "[" << logging::trivial::severity << "] " << expr::smessage);
    logging::core::get()->add_sink(c.sink);
}

void stop_log()
{
    LogCapture &c = capture();
    if (!c.sink)
        return;
    c.sink->flush();
    boost::log::core::get()->remove_sink(c.sink);
    c.sink.reset();
}

std::string take_log()
{
    LogCapture &c = capture();
    if (c.sink)
        c.sink->flush();
    std::string s = c.text.str();
    c.text.str(std::string());
    c.text.clear();
    return s;
}

// ---------------------------------------------------------------- shapes

namespace {

Polygon poly(std::initializer_list<Point> pts) { return Polygon(Points(pts)); }

Polygon rect(coord_t x0, coord_t y0, coord_t x1, coord_t y1) { return poly({{x0, y0}, {x1, y0}, {x1, y1}, {x0, y1}}); }

// A shape of polygons kept as given, each a contour of its own (their orientation unchanged).
ExPolygons as_contours(const Polygons &polys)
{
    ExPolygons out;
    for (const Polygon &q : polys)
        out.emplace_back(q);
    return out;
}

ExPolygon with_holes(const Polygon &contour, const Polygons &holes)
{
    ExPolygon e(contour);
    e.holes = holes;
    return e;
}

// A star around (cx, cy): n points at angles 2 pi (i + [0, 0.8)) / n and radii in [r_lo, r_hi]
// times r, counter-clockwise; clockwise when `cw`.
Polygon star(Gen &g, coord_t cx, coord_t cy, double r, size_t n, double r_lo, double r_hi, bool cw)
{
    Points pts;
    for (size_t i = 0; i < n; ++i) {
        const double a  = 2. * PI * (double(i) + g.real(0., 0.8)) / double(n);
        const double rr = g.real(r_lo, r_hi) * r;
        pts.emplace_back(cx + coord_t(std::llround(rr * std::cos(a))), cy + coord_t(std::llround(rr * std::sin(a))));
    }
    if (cw)
        std::reverse(pts.begin(), pts.end());
    Points out;
    for (const Point &q : pts)
        if (out.empty() || q != out.back())
            out.push_back(q);
    while (out.size() > 1 && out.front() == out.back())
        out.pop_back();
    return Polygon(out);
}

// The 12707 polygon of test_voronoi.cpp, turned as its test turns it.
Polygon poly_12707()
{
    Polygon      q = poly({{0, 10000000}, {700000, 1}, {700000, 9000000}, {9100000, 9000000}, {9100000, 0}, {10000000, 10000000}});
    const double a = 0.18764587962597876897475f;
    const double c = std::cos(a);
    const double s = std::sin(a);
    for (Point &p : q.points) {
        const Vec2d v = p.cast<double>();
        p.x()         = coord_t(v.x() * c + v.y() * s + 0.5);
        p.y()         = coord_t(-v.x() * s + v.y() * c + 0.5);
    }
    return q;
}

Polygon poly5()
{
    return poly({{35058884, -25732145}, {35058884, -19586070}, {32753739, -20246796}, {28756244, -21010725}, {24657532, -21657939}, {20836260, -21960233},
                 {16115145, -22070742}, {11850152, -21839761}, {7646240, -21470177},   {3607605, -20786940},   {1280947, -20329742},   {-292823, -19963790},
                 {-3844469, -18809741}, {-7237277, -17593723},  {-10225900, -16143761}, {-13030266, -14643721}, {-15404294, -12977561}, {-17601713, -11280712},
                 {-19241930, -9435607}, {-20714420, -7583739},  {-21726144, -5664355},  {-22579294, -3741947},  {-22966684, -1786321},  {-23200322, 170140},
                 {-22966684, 2126602},  {-22579296, 4082227},   {-21726148, 6004637},   {-20714424, 7924020},   {-19241932, 9775888},   {-17601717, 11620994},
                 {-15404423, 13317749}, {-13030276, 14984003},  {-10225910, 16484042},  {-7237288, 17934005},   {-3844482, 19150025},   {-292841, 20304074},
                 {1280949, 20670031},   {3607587, 21127226},    {7646218, 21810465},    {11850128, 22180055},   {16115122, 22411036},   {20836263, 22300531},
                 {24657513, 21998239},  {28756227, 21351025},   {32753725, 20587092},   {35058893, 19926309},   {35058893, 35000000},   {-31657232, 35000000},
                 {-31657202, -35000000}, {35058881, -35000000}});
}

// The OBJ meshes of src/tests/data, from the oracle's source tree (<exe>/../src/tests/data, as
// model.cpp reads them): vertices and faces, each face fanned from its first vertex.
bool read_obj(const std::filesystem::path &path, indexed_triangle_set &its)
{
    std::ifstream in(path, std::ios::binary);
    if (!in)
        return false;
    std::string line;
    while (std::getline(in, line)) {
        if (!line.empty() && line.back() == '\r')
            line.pop_back();
        if (line.size() > 2 && line[0] == 'v' && line[1] == ' ') {
            std::istringstream s(line.substr(2));
            float              x = 0, y = 0, z = 0;
            s >> x >> y >> z;
            its.vertices.emplace_back(x, y, z);
        } else if (line.size() > 2 && line[0] == 'f' && line[1] == ' ') {
            std::istringstream s(line.substr(2));
            std::vector<int>   idx;
            std::string        tok;
            while (s >> tok)
                idx.push_back(std::atoi(tok.c_str()) - 1);
            for (size_t i = 2; i < idx.size(); ++i)
                its.indices.emplace_back(idx[0], idx[i - 1], idx[i]);
        }
    }
    return !its.vertices.empty() && !its.indices.empty();
}

std::filesystem::path test_data()
{
    char *pgm = nullptr;
    if (_get_pgmptr(&pgm) != 0 || pgm == nullptr)
        return {};
    return std::filesystem::path(pgm).parent_path().parent_path() / "src" / "tests" / "data";
}

const char *const slice_meshes[] = {"20mm_cube",      "A",           "V",        "bridge",        "cube_with_concave_hole", "cube_with_hole", "extruder_idler",
                                    "frog_legs",      "ipadstand",   "overhang", "sloping_hole",  "two_hollow_squares",     "simplification", "small_dorito",
                                    "pyramid",        "2x20x10"};

// The meshes read_obj could not read, by path: inputs_ok fails the probe on the first.
std::vector<std::string> &unread_meshes()
{
    static std::vector<std::string> v;
    return v;
}

void mesh_slices(std::vector<Shape> &out)
{
    const std::filesystem::path data = test_data();
    tbb::global_control         one(tbb::global_control::max_allowed_parallelism, 1);
    for (const char *name : slice_meshes) {
        indexed_triangle_set        its;
        const std::filesystem::path path = data / (std::string(name) + ".obj");
        if (!read_obj(path, its)) {
            unread_meshes().push_back(path.string());
            continue;
        }
        float zmin = its.vertices.front().z(), zmax = zmin;
        for (const auto &v : its.vertices) {
            zmin = std::min(zmin, v.z());
            zmax = std::max(zmax, v.z());
        }
        const float fractions[] = {0.13f, 0.5f, 0.77f};
        std::vector<float> zs;
        for (float f : fractions)
            zs.push_back(zmin + (zmax - zmin) * f);
        const std::vector<ExPolygons> layers = slice_mesh_ex(its, zs, MeshSlicingParamsEx{});
        for (size_t i = 0; i < layers.size(); ++i)
            if (!layers[i].empty())
                out.push_back({std::string("slice ") + name + " z" + std::to_string(i), layers[i], false});
    }
}

std::vector<Shape> make_shapes()
{
    std::vector<Shape> out;
    // Hand-made.
    out.push_back({"square 10 mm", {ExPolygon(rect(0, 0, 10000000, 10000000))}, true});
    out.push_back({"square with a square hole", {with_holes(rect(0, 0, 10000000, 10000000), {rect(3000000, 3000000, 7000000, 7000000)})}, true});
    {
        Polygon h = rect(3000000, 3000000, 7000000, 7000000);
        h.reverse();
        out.push_back({"square with a clockwise hole", {with_holes(rect(0, 0, 10000000, 10000000), {h})}, true});
    }
    out.push_back({"triangle", {ExPolygon(poly({{0, 0}, {20000000, 0}, {5000000, 9000000}}))}, true});
    out.push_back({"thin sliver", {ExPolygon(poly({{0, 0}, {20000000, 0}, {20000000, 300000}, {0, 50000}}))}, true});
    out.push_back({"L shape", {ExPolygon(poly({{0, 0}, {8000000, 0}, {8000000, 2000000}, {2000000, 2000000}, {2000000, 8000000}, {0, 8000000}}))}, true});
    out.push_back({"collinear points", {ExPolygon(poly({{0, 0}, {5000000, 0}, {10000000, 0}, {10000000, 10000000}, {5000000, 10000000}, {0, 10000000}}))}, true});
    out.push_back({"repeated point", {ExPolygon(poly({{0, 0}, {10000000, 0}, {10000000, 0}, {10000000, 10000000}, {0, 10000000}}))}, true});
    // Two nanometres apart, as SPE-1729's thin lines and holes are (MedialAxis::build).
    out.push_back({"nano hole", {with_holes(rect(0, 0, 1000000, 1000000), {poly({{500000, 2}, {400000, 200000}, {600000, 200000}})})}, true});
    out.push_back({"nano slit", {ExPolygon(poly({{0, 0}, {499999, 0}, {499999, 600000}, {500002, 600000}, {500002, 0}, {1000000, 0}, {1000000, 1000000}, {0, 1000000}}))}, true});
    // The issue polygons of test_voronoi.cpp.
    out.push_back({"missing vertex 1", {ExPolygon(poly({{25000000, 25000000}, {-25000000, 25000000}, {-25000000, -25000000}, {-12412500, -25000000}, {25000000, -25000000}}))}, true});
    out.push_back({"missing vertex 2",
                   {with_holes(poly({{50000000, 50000000}, {-50000000, 50000000}, {-50000000, -50000000}, {50000000, -50000000}}),
                               {poly({{-45000000, -45000000}, {-45000000, 45000000}, {45000000, 45000000}, {45000000, 8280000}, {45000000, -45000000}})})},
                   true});
    out.push_back({"missing vertex 3",
                   as_contours({poly({{-29715088, -29310899}, {-29022573, -28618384}, {-27771147, -27366958}, {-28539221, -26519393}, {-30619013, -28586348}, {-29812018, -29407830}}),
                                poly({{-27035112, -28071875}, {-27367482, -27770679}, {-28387008, -28790205}, {-29309438, -29712635}, {-29406319, -29809515}, {-29032985, -30179156}})}),
                   true});
    out.push_back({"duplicate vertices",
                   {ExPolygon(poly({{25000000, 25000000}, {-25000000, 25000000}, {-25000000, -25000000}, {146872, -25000000}, {9912498, -25000000}, {25000000, -25000000},
                                    {25000000, -8056252}, {25000000, -146873}, {25000000, 10790627}}))},
                   true});
    out.push_back({"intersecting edges",
                   {ExPolygon(poly({{25000000, 25000000}, {-25000000, 25000000}, {-25000000, -25000000}, {146872, -25000000}, {25000000, -25000000}, {25000000, -146873}}))},
                   true});
    out.push_back({"gapfill 12707", {ExPolygon(poly_12707())}, true});
    out.push_back({"weirdness poly2", {ExPolygon(poly({{0, 0}, {70000000, 0}, {70000000, 1300000}, {70700000, 14000000}}))}, true});
    out.push_back({"weirdness poly5", {ExPolygon(poly5())}, true});
    out.push_back({"weirdness poly7", {ExPolygon(poly({{35058884, -25732145}, {35058884, -19586070}, {-31657202, -35000000}, {35058881, -35000000}}))}, true});
    // Slices of the test meshes.
    mesh_slices(out);
    // Seeded stars, with and without holes.
    Gen          g(0x57A125u);
    const double radii[] = {2000., 100000., 1000000., 10000000., 100000000.};
    for (int i = 0; i < 48; ++i) {
        const double  r  = radii[g.range(0, 4)];
        const coord_t cx = coord_t(g.range(-1000000, 1000000)), cy = coord_t(g.range(-1000000, 1000000));
        const size_t  n  = size_t(g.range(5, 40));
        ExPolygon     e(star(g, cx, cy, r, n, 0.5, 1.0, false));
        if (g.chance(2))
            e.holes.push_back(star(g, cx, cy, r, size_t(g.range(5, 16)), 0.05, 0.15, true));
        out.push_back({"star " + std::to_string(i), {e}, false});
    }
    return out;
}

// The thin-wall regions PerimeterGenerator gives the medial axis of a region (classic walls, its
// thin walls: offsets = offset2_ex(last, -(w/2 + s/2 - 1), s/2 - 1), then opening_ex(diff_ex(last,
// offset(offsets, w/2 + ClipperSafetyOffset)), min/2), widths min = nozzle / 3 and max = w + s)
// and the gap-fill regions it and the fill give it (diff_ex(opening_ex(gaps, min/2),
// offset2_ex(gaps, -max/2, max/2 + ClipperSafetyOffset)), min = 0.2 w (1 - 0.15), max = 2 s), the
// gaps here being what lies between an inner offset of the region and its opening.
void derived_thin(const Shape &shape, std::vector<ThinCase> &out, size_t limit)
{
    struct Flow
    {
        double w, s, nozzle;
    };
    const Flow flows[] = {{420000., 380000., 400000.}, {650000., 590000., 600000.}};
    size_t     taken   = 0;
    for (const Flow &f : flows) {
        const ExPolygons offsets = offset2_ex(shape.ex, -float(f.w / 2. + f.s / 2. - 1.), float(f.s / 2. - 1.));
        const double     min_w   = f.nozzle / 3.;
        const ExPolygons thin    = opening_ex(diff_ex(shape.ex, offset(offsets, float(f.w / 2.) + ClipperSafetyOffset)), float(min_w / 2.));
        for (const ExPolygon &e : thin)
            if (taken++ < limit)
                out.push_back({shape.label + " thin wall " + std::to_string(int(f.w)), e, min_w, f.w + f.s, false});
        const ExPolygons inner   = offset_ex(shape.ex, -float(f.s / 2.));
        const ExPolygons gaps    = diff_ex(inner, opening_ex(inner, float(f.s)));
        const double     gmin    = 0.2 * f.w * (1. - 0.15);
        const double     gmax    = 2. * f.s;
        const ExPolygons gaps_ex = diff_ex(opening_ex(gaps, float(gmin / 2.)), offset2_ex(gaps, -float(gmax / 2.), float(gmax / 2. + ClipperSafetyOffset)));
        for (const ExPolygon &e : gaps_ex)
            if (taken++ < limit)
                out.push_back({shape.label + " gap fill " + std::to_string(int(f.w)), e, gmin, gmax, false});
    }
}

std::vector<ThinCase> make_thin_cases()
{
    std::vector<ThinCase> out;
    const double          mn = 133333., mx = 800000.;
    out.push_back({"strip 0.3 mm", ExPolygon(rect(0, 0, 10000000, 300000)), mn, mx, true});
    out.push_back({"strip 0.1 mm", ExPolygon(rect(0, 0, 10000000, 100000)), mn, mx, true});
    out.push_back({"strip 1.2 mm", ExPolygon(rect(0, 0, 10000000, 1200000)), mn, mx, true});
    out.push_back({"strip 3 nm", ExPolygon(rect(0, 0, 10000000, 3)), mn, mx, true});
    out.push_back({"wedge", ExPolygon(poly({{0, 0}, {10000000, -400000}, {10000000, 400000}})), mn, mx, true});
    out.push_back({"L strip", ExPolygon(poly({{0, 0}, {5000000, 0}, {5000000, 400000}, {400000, 400000}, {400000, 5000000}, {0, 5000000}})), mn, mx, true});
    out.push_back({"T strip",
                   ExPolygon(poly({{0, 0}, {8000000, 0}, {8000000, 400000}, {4200000, 400000}, {4200000, 6000000}, {3800000, 6000000}, {3800000, 400000}, {0, 400000}})), mn, mx,
                   true});
    out.push_back({"cross strip",
                   ExPolygon(poly({{-200000, -4000000}, {200000, -4000000}, {200000, -200000}, {4000000, -200000}, {4000000, 200000}, {200000, 200000},
                                   {200000, 4000000}, {-200000, 4000000}, {-200000, 200000}, {-4000000, 200000}, {-4000000, -200000}, {-200000, -200000}})),
                   mn, mx, true});
    {
        Polygon h = rect(300000, 300000, 9700000, 9700000);
        h.reverse();
        out.push_back({"square ring 0.3 mm", with_holes(rect(0, 0, 10000000, 10000000), {h}), mn, mx, true});
    }
    {
        Polygon c, h;
        for (int i = 0; i < 64; ++i) {
            const double a = 2. * PI * i / 64.;
            c.points.emplace_back(coord_t(std::llround(5000000. * std::cos(a))), coord_t(std::llround(5000000. * std::sin(a))));
            h.points.emplace_back(coord_t(std::llround(4650000. * std::cos(a))), coord_t(std::llround(4650000. * std::sin(a))));
        }
        h.reverse();
        out.push_back({"round ring 0.35 mm", with_holes(c, {h}), mn, mx, true});
    }
    out.push_back({"zigzag strip",
                   ExPolygon(poly({{0, 0}, {2000000, 1000000}, {4000000, 0}, {6000000, 1000000}, {6000000, 1400000}, {4000000, 400000}, {2000000, 1400000}, {0, 400000}})), mn,
                   mx, true});
    out.push_back({"tapered strip", ExPolygon(poly({{0, 0}, {10000000, 0}, {10000000, 900000}, {0, 100000}})), 90000., 900000., true});
    out.push_back({"strip with a nano hole", with_holes(rect(0, 0, 10000000, 400000), {poly({{5000000, 2}, {4900000, 100000}, {5100000, 100000}})}), mn, mx, true});
    out.push_back({"strip 0.3 mm, narrow widths", ExPolygon(rect(0, 0, 10000000, 300000)), 290000., 310000., true});
    // The regions of the shapes.
    for (const Shape &s : shapes())
        derived_thin(s, out, s.hand ? 6 : 4);
    // Seeded thin rings.
    Gen g(0x7E1A9u);
    for (int i = 0; i < 40; ++i) {
        const double  r  = g.real(2000000., 30000000.);
        const size_t  n  = size_t(g.range(5, 32));
        const Polygon st = star(g, 0, 0, r, n, 0.6, 1.0, false);
        const double  w  = g.real(100000., 1000000.);
        for (const ExPolygon &e : diff_ex(ExPolygons{ExPolygon(st)}, offset_ex(ExPolygons{ExPolygon(st)}, -float(w))))
            out.push_back({"ring " + std::to_string(i), e, w * g.real(0.2, 0.6), w * g.real(1.1, 2.5), false});
    }
    return out;
}

bool fits_int32(const Point &q) { return q.x() >= INT32_MIN && q.x() <= INT32_MAX && q.y() >= INT32_MIN && q.y() <= INT32_MAX; }

Pt32 p32(int64_t x, int64_t y) { return Pt32{int32_t(x), int32_t(y)}; }

std::vector<SiteSet> make_site_sets()
{
    std::vector<SiteSet> out;
    auto                 add = [&](const std::string &label, std::vector<Pt32> pts, std::vector<Seg32> segs) { out.push_back({label, std::move(pts), std::move(segs), true}); };
    add("empty", {}, {});
    add("one point", {{0, 0}}, {});
    add("one segment", {}, {{{0, 0}, {10, 5}}});
    add("two points", {{0, 0}, {10, 0}}, {});
    add("two points on a vertical", {{0, 0}, {0, 10}}, {});
    add("vertical collinear points", {{0, 0}, {0, 10}, {0, 20}, {0, 30}}, {});
    add("horizontal collinear points", {{0, 0}, {10, 0}, {20, 0}, {30, 0}}, {});
    add("vertical points then one off", {{0, 0}, {0, 10}, {0, 20}, {7, 5}}, {});
    add("square corners", {{0, 0}, {10, 0}, {10, 10}, {0, 10}}, {});
    {
        std::vector<Pt32> g3, g5;
        for (int i = 0; i < 3; ++i)
            for (int j = 0; j < 3; ++j)
                g3.push_back({i * 10, j * 10});
        for (int i = 0; i < 5; ++i)
            for (int j = 0; j < 5; ++j)
                g5.push_back({i * 7 - 14, j * 7 - 14});
        add("grid 3 by 3", g3, {});
        add("grid 5 by 5", g5, {});
    }
    add("duplicate points", {{1, 1}, {1, 1}, {2, 3}, {2, 3}, {5, 0}}, {});
    add("points 12067", {{-10, -20}, {10, -20}, {5, 0}, {10, 20}, {-10, 20}, {-5, 0}}, {});
    add("points 12903", {{1, 1}, {3, 1}, {1, 3}, {3, 3}, {-1, 1}, {1, -1}, {5, 1}, {3, -1}, {-1, 3}, {1, 5}, {5, 3}, {3, 5}}, {});
    add("zero-length segment", {{20, 20}}, {{{5, 5}, {5, 5}}, {{0, 10}, {10, 10}}});
    add("segment and point", {{5, 5}}, {{{0, 0}, {10, 0}}});
    add("segment chain", {}, {{{0, 0}, {10, 0}}, {{10, 0}, {10, 10}}, {{10, 10}, {0, 10}}});
    add("triangle", {}, {{{0, 0}, {100, 0}}, {{100, 0}, {30, 80}}, {{30, 80}, {0, 0}}});
    add("reversed segments", {}, {{{10, 0}, {0, 0}}, {{10, 10}, {10, 0}}, {{0, 10}, {10, 10}}, {{0, 0}, {0, 10}}});
    add("vertical segments", {}, {{{0, 0}, {0, 10}}, {{5, 0}, {5, 10}}, {{10, 0}, {10, 10}}, {{5, 20}, {5, 30}}});
    add("collinear vertical segments", {}, {{{0, 0}, {0, 10}}, {{0, 10}, {0, 20}}, {{0, 30}, {0, 40}}});
    add("collinear horizontal segments", {}, {{{0, 0}, {10, 0}}, {{10, 0}, {20, 0}}, {{30, 0}, {40, 0}}});
    {
        std::vector<Seg32> spokes;
        for (int i = 0; i < 8; ++i) {
            const double a = 2. * PI * i / 8.;
            spokes.push_back({{0, 0}, p32(std::llround(1000. * std::cos(a)), std::llround(1000. * std::sin(a)))});
        }
        add("spokes", {}, spokes);
    }
    add("nested squares", {},
        {{{0, 0}, {100, 0}}, {{100, 0}, {100, 100}}, {{100, 100}, {0, 100}}, {{0, 100}, {0, 0}}, {{30, 30}, {30, 70}}, {{30, 70}, {70, 70}}, {{70, 70}, {70, 30}}, {{70, 30}, {30, 30}}});
    add("square with points inside", {{50, 50}, {20, 80}, {75, 10}},
        {{{0, 0}, {100, 0}}, {{100, 0}, {100, 100}}, {{100, 100}, {0, 100}}, {{0, 100}, {0, 0}}});
    add("large coordinates", {{0, 0}},
        {{{-2147483000, -2147483000}, {2147483000, -2147483000}}, {{2147483000, -2147483000}, {2147483000, 2147483000}}, {{2147483000, 2147483000}, {-2147483000, 2147483000}},
         {{-2147483000, 2147483000}, {-2147483000, -2147483000}}});
    add("large points", {{-2147483647, -2147483647}, {2147483647, 2147483647}, {2147483647, -2147483647}, {-2147483647, 2147483647}, {1, -3}}, {});
    add("nearly collinear points", {{0, 0}, {1000000000, 1}, {2000000000, 3}, {-1000000000, -1}, {500000000, 0}}, {});
    // The shapes' contours, as segments.
    for (const Shape &s : shapes()) {
        Lines lines = to_lines(s.ex);
        if (lines.size() > 400)
            continue;
        bool ok = true;
        for (const Line &l : lines)
            ok = ok && fits_int32(l.a) && fits_int32(l.b);
        if (!ok)
            continue;
        SiteSet ss{"contours of " + s.label, {}, {}, s.hand};
        for (const Line &l : lines)
            ss.segments.push_back({p32(l.a.x(), l.a.y()), p32(l.b.x(), l.b.y())});
        out.push_back(std::move(ss));
    }
    // Seeded: point clouds, grids with holes, collinear runs, and polygons with points inside.
    Gen           g(0x5EED5E7u);
    const int64_t ranges[] = {10, 1000, 1000000, 1073741823};
    for (int i = 0; i < 60; ++i) {
        SiteSet       ss{"points " + std::to_string(i), {}, {}, false};
        const int64_t r = ranges[g.range(0, 3)];
        const int     n = int(g.range(3, 60));
        for (int k = 0; k < n; ++k)
            ss.points.push_back(p32(g.range(-r, r), g.range(-r, r)));
        out.push_back(std::move(ss));
    }
    for (int i = 0; i < 20; ++i) {
        SiteSet       ss{"grid " + std::to_string(i), {}, {}, false};
        const int64_t step = g.range(1, 1000000);
        const int     n    = int(g.range(2, 8));
        for (int a = 0; a < n; ++a)
            for (int b = 0; b < n; ++b)
                if (!g.chance(4))
                    ss.points.push_back(p32(a * step, b * step));
        out.push_back(std::move(ss));
    }
    for (int i = 0; i < 20; ++i) {
        SiteSet       ss{"collinear " + std::to_string(i), {}, {}, false};
        const int64_t dx = g.range(-1000, 1000), dy = g.range(-1000, 1000);
        const int     n  = int(g.range(3, 20));
        for (int k = 0; k < n; ++k)
            ss.points.push_back(p32(dx * k, dy * k));
        if (g.chance(2))
            ss.points.push_back(p32(g.range(-5000, 5000), g.range(-5000, 5000)));
        out.push_back(std::move(ss));
    }
    for (int i = 0; i < 40; ++i) {
        SiteSet       ss{"polygon " + std::to_string(i), {}, {}, false};
        const double  r  = double(ranges[g.range(1, 3)]) * 0.9 + 2000.;
        const coord_t cx = coord_t(g.range(-1000, 1000)), cy = coord_t(g.range(-1000, 1000));
        Polygons      pp{star(g, cx, cy, r, size_t(g.range(5, 40)), 0.5, 1.0, g.chance(3))};
        if (g.chance(2))
            pp.push_back(star(g, cx, cy, r, size_t(g.range(5, 12)), 0.05, 0.15, true));
        for (const Line &l : to_lines(pp))
            ss.segments.push_back({p32(l.a.x(), l.a.y()), p32(l.b.x(), l.b.y())});
        if (pp.size() == 1) {
            const int n = int(g.range(0, 4));
            for (int k = 0; k < n; ++k)
                ss.points.push_back(p32(cx + coord_t(g.real(-0.3, 0.3) * r), cy + coord_t(g.real(-0.3, 0.3) * r)));
        }
        out.push_back(std::move(ss));
    }
    return out;
}

} // namespace

const std::vector<Shape> &shapes()
{
    static const std::vector<Shape> s = make_shapes();
    return s;
}

const std::vector<SiteSet> &site_sets()
{
    static const std::vector<SiteSet> s = make_site_sets();
    return s;
}

bool inputs_ok(std::string &error)
{
    shapes();
    if (unread_meshes().empty())
        return true;
    error = "voronoi: cannot read the test mesh " + unread_meshes().front();
    return false;
}

const std::vector<ThinCase> &thin_cases()
{
    static const std::vector<ThinCase> s = make_thin_cases();
    return s;
}

Polygons arachne_polygons(const ExPolygons &ex)
{
    Polygons polys = to_polygons(ex);
    for (Polygon &q : polys) {
        Points pts;
        for (const Point &x : q.points)
            if (pts.empty() || x != pts.back())
                pts.push_back(x);
        while (pts.size() > 1 && pts.front() == pts.back())
            pts.pop_back();
        q.points = pts;
    }
    remove_collinear(polys);
    polys.erase(std::remove_if(polys.begin(), polys.end(), [](const Polygon &q) { return q.points.size() < 3; }), polys.end());
    return polys;
}

size_t segment_count(const Polygons &polys)
{
    size_t n = 0;
    for (const Polygon &q : polys)
        n += q.points.size();
    return n;
}

} // namespace probe::vr
