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
// The fill probe's shared pieces (fill.hpp says what each is; orca-o3-probe-fill, 2026-11-13).
// The values are written as parity/sop1/fill.md ("fill.sop1's values") and parity/SOP1.md
// ("Values") describe them; an entity is SOP1.md's, as the oracle's dumps write it.
#include "fill.hpp"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <typeinfo>

#include <fcntl.h>
#include <io.h>
#include <process.h>
#include <stdlib.h>

#include <boost/core/null_deleter.hpp>
#include <boost/log/core.hpp>
#include <boost/log/expressions.hpp>
#include <boost/log/trivial.hpp>
#include <boost/make_shared.hpp>

#include "libslic3r/ClipperUtils.hpp"
#include "libslic3r/Format/OBJ.hpp"
#include "libslic3r/TriangleMesh.hpp"
#include "libslic3r/TriangleMeshSlicer.hpp"

namespace probe {
namespace fill {

using namespace Slic3r;

// ---------------------------------------------------------------- values

void put(Payload &p, const Point &q) { p.i64(int64_t(q.x())).i64(int64_t(q.y())); }

void put_points(Payload &p, const Points &pts)
{
    p.u64(pts.size());
    for (const Point &q : pts)
        put(p, q);
}

void put(Payload &p, const Polygon &q) { put_points(p, q.points); }

void put(Payload &p, const Polygons &qs)
{
    p.u64(qs.size());
    for (const Polygon &q : qs)
        put(p, q);
}

void put(Payload &p, const ExPolygon &e)
{
    put(p, e.contour);
    put(p, e.holes);
}

void put(Payload &p, const ExPolygons &es)
{
    p.u64(es.size());
    for (const ExPolygon &e : es)
        put(p, e);
}

namespace {

void put_fitting(Payload &p, const std::vector<PathFittingData> &fitting)
{
    p.u64(fitting.size());
    for (const PathFittingData &f : fitting) {
        p.u64(uint64_t(f.start_point_index));
        p.u64(uint64_t(f.end_point_index));
        p.u8(uint8_t(f.path_type));
        const ArcSegment &arc = f.arc_data;
        p.boolean(arc.is_arc);
        put(p, arc.center);
        p.f64(arc.radius);
        p.f64(arc.length);
        p.f64(arc.angle_radians);
        p.f64(arc.polar_start_theta);
        p.f64(arc.polar_end_theta);
        put(p, arc.start_point);
        put(p, arc.end_point);
        p.u8(uint8_t(arc.direction));
    }
}

} // namespace

void put(Payload &p, const Polyline &pl)
{
    put_points(p, pl.points);
    put_fitting(p, pl.fitting_result);
}

void put(Payload &p, const Polylines &pls)
{
    p.u64(pls.size());
    for (const Polyline &pl : pls)
        put(p, pl);
}

void put(Payload &p, const ThickPolyline &t)
{
    put_points(p, t.points);
    put_fitting(p, t.fitting_result);
    p.u64(t.width.size());
    for (double w : t.width)
        p.f64(w);
    p.boolean(t.endpoints.first);
    p.boolean(t.endpoints.second);
}

void put(Payload &p, const ThickPolylines &ts)
{
    p.u64(ts.size());
    for (const ThickPolyline &t : ts)
        put(p, t);
}

void put(Payload &p, const BoundingBox &b)
{
    put(p, b.min);
    put(p, b.max);
    p.boolean(b.defined);
}

void put(Payload &p, const Surface &s)
{
    p.u32(uint32_t(s.surface_type));
    put(p, s.expolygon);
    p.f64(s.thickness);
    p.u16(uint16_t(s.thickness_layers));
    p.f64(s.bridge_angle);
    p.u16(uint16_t(s.extra_perimeters));
}

void put(Payload &p, const Flow &f)
{
    p.f32(f.width()).f32(f.height()).f32(f.spacing()).f32(f.nozzle_diameter()).boolean(f.bridge());
}

void put(Payload &p, const FillParams &q)
{
    p.f32(q.density).i32(int32_t(q.multiline)).f32(q.anchor_length).f32(q.anchor_length_max).f64(q.resolution);
    p.boolean(q.dont_adjust).boolean(q.monotonic).boolean(q.complete).boolean(q.use_arachne).f64(q.layer_height);
    p.boolean(q.gyroid_optimized).f64(q.lateral_lattice_angle_1).f64(q.lateral_lattice_angle_2).i32(int32_t(q.pattern));
    p.f32(q.infill_overhang_angle);
    put(p, q.flow);
    p.u8(uint8_t(q.extrusion_role)).boolean(q.using_internal_flow).f32(q.no_extrusion_overlap).boolean(q.config != nullptr);
    p.boolean(q.dont_sort).boolean(q.can_reverse).f32(q.horiz_move).boolean(q.symmetric_infill_y_axis).i64(int64_t(q.symmetric_y_axis));
    p.boolean(q.locked_zag).f32(q.infill_lock_depth).f32(q.skin_infill_depth);
}

namespace {

// The entity kinds of parity/SOP1.md ("Values"), the first byte of each entity.
enum EntityKind : uint8_t {
    ekUnknown       = 0,
    ekPath          = 1,
    ekPathContoured = 2,
    ekPathSloped    = 3,
    ekPathOriented  = 4,
    ekMultiPath     = 5,
    ekLoop          = 6,
    ekLoopSloped    = 7,
    ekCollection    = 8,
};

void put_path_fields(Payload &p, const ExtrusionPath &path)
{
    p.u8(uint8_t(path.role()));
    p.i32(int32_t(path.inset_idx));
    p.u64(path.polyline.points.size());
    for (const Point3 &pt : path.polyline.points)
        p.i64(int64_t(pt.x())).i64(int64_t(pt.y())).i64(int64_t(pt.z()));
    put_fitting(p, path.polyline.fitting_result);
    p.f64(path.overhang_degree).i32(int32_t(path.curve_degree)).f64(path.mm3_per_mm).f32(path.width).f32(path.height);
    p.f64(path.smooth_speed).boolean(path.z_contoured).boolean(path.is_force_no_extrusion()).boolean(path.can_reverse());
}

void put_slope(Payload &p, const ExtrusionPathSloped &path)
{
    p.f64(path.slope_begin.z_ratio).f64(path.slope_begin.e_ratio).f64(path.slope_end.z_ratio).f64(path.slope_end.e_ratio);
}

void put_loop_fields(Payload &p, const ExtrusionLoop &loop)
{
    p.i32(int32_t(loop.inset_idx));
    p.u8(uint8_t(loop.loop_role()));
    p.u64(loop.paths.size());
    for (const ExtrusionPath &path : loop.paths)
        put_entity(p, path);
}

} // namespace

void put_entity(Payload &p, const ExtrusionEntity &entity)
{
    const std::type_info &type = typeid(entity);
    if (type == typeid(ExtrusionPath)) {
        p.u8(ekPath);
        put_path_fields(p, static_cast<const ExtrusionPath &>(entity));
    } else if (type == typeid(ExtrusionPathContoured)) {
        const auto &path = static_cast<const ExtrusionPathContoured &>(entity);
        p.u8(ekPathContoured);
        put_path_fields(p, path);
        p.u64(path.z_diffs.size());
        for (double z : path.z_diffs)
            p.f64(z);
    } else if (type == typeid(ExtrusionPathSloped)) {
        const auto &path = static_cast<const ExtrusionPathSloped &>(entity);
        p.u8(ekPathSloped);
        put_path_fields(p, path);
        put_slope(p, path);
    } else if (type == typeid(ExtrusionPathOriented)) {
        p.u8(ekPathOriented);
        put_path_fields(p, static_cast<const ExtrusionPath &>(entity));
    } else if (type == typeid(ExtrusionMultiPath)) {
        const auto &multi = static_cast<const ExtrusionMultiPath &>(entity);
        p.u8(ekMultiPath);
        p.i32(int32_t(multi.inset_idx));
        p.boolean(multi.can_reverse());
        p.u64(multi.paths.size());
        for (const ExtrusionPath &path : multi.paths)
            put_entity(p, path);
    } else if (type == typeid(ExtrusionLoop)) {
        p.u8(ekLoop);
        put_loop_fields(p, static_cast<const ExtrusionLoop &>(entity));
    } else if (type == typeid(ExtrusionLoopSloped)) {
        const auto &loop = static_cast<const ExtrusionLoopSloped &>(entity);
        p.u8(ekLoopSloped);
        put_loop_fields(p, loop);
        p.u64(loop.starts.size());
        for (const ExtrusionPathSloped &path : loop.starts)
            put_entity(p, path);
        p.u64(loop.ends.size());
        for (const ExtrusionPathSloped &path : loop.ends)
            put_entity(p, path);
    } else if (type == typeid(ExtrusionEntityCollection)) {
        const auto &collection = static_cast<const ExtrusionEntityCollection &>(entity);
        p.u8(ekCollection);
        p.i32(int32_t(collection.inset_idx));
        p.boolean(collection.no_sort);
        p.boolean(collection.can_reverse());
        p.u64(collection.entities.size());
        for (const ExtrusionEntity *child : collection.entities)
            put_entity(p, *child);
    } else {
        p.u8(ekUnknown);
        p.str(type.name());
    }
}

void put(Payload &p, const ExtrusionEntitiesPtr &es)
{
    p.u64(es.size());
    for (const ExtrusionEntity *e : es)
        put_entity(p, *e);
}

void put(Payload &p, const Configs &c)
{
    p.u64(c.print.nozzle_diameter.values.size());
    for (double d : c.print.nozzle_diameter.values)
        p.f64(d);
    p.i32(int32_t(c.object.gap_fill_target.value));
    p.f64(c.region.filter_out_gap_fill.value);
}

void put_filler(Payload &p, const Fill &f)
{
    p.u64(uint64_t(f.layer_id)).f64(f.z).f64(f.spacing).f64(f.overlap).f32(f.angle).boolean(f.fixed_angle);
    p.i64(int64_t(f.link_max_length)).i64(int64_t(f.loop_clipping));
    put(p, f.bounding_box);
    p.boolean(f.dont_alternate_fill_direction);
    put(p, f.no_overlap_expolygons);
    p.boolean(f.print_object_config != nullptr);
}

Entities::~Entities()
{
    for (ExtrusionEntity *e : v)
        delete e;
}

void FillerState::apply(Fill &f, const Configs &c) const
{
    f.layer_id                      = layer_id;
    f.z                             = z;
    f.spacing                       = spacing;
    f.overlap                       = overlap;
    f.angle                         = angle;
    f.fixed_angle                   = fixed;
    f.link_max_length               = coord_t(link_max);
    f.loop_clipping                 = coord_t(loop_clip);
    f.bounding_box                  = bbox;
    f.dont_alternate_fill_direction = dont_alt;
    f.no_overlap_expolygons         = no_overlap;
    f.print_config                  = configs ? &c.print : nullptr;
    f.print_object_config           = configs ? &c.object : nullptr;
}

void put_threw(Payload &p, const std::exception &e)
{
    p.u8(1);
    p.str(e.what());
}

// ---------------------------------------------------------------- shapes

int64_t mm(double v) { return int64_t(scale_(v)); }

Polygon rect(double x0, double y0, double w, double h)
{
    return Polygon({Point(mm(x0), mm(y0)), Point(mm(x0 + w), mm(y0)), Point(mm(x0 + w), mm(y0 + h)), Point(mm(x0), mm(y0 + h))});
}

Polygon ngon(double cx, double cy, double r, int n, double phase)
{
    Polygon out;
    for (int i = 0; i < n; ++i) {
        const double a = phase + 2. * PI * double(i) / double(n);
        out.points.emplace_back(mm(cx + r * std::cos(a)), mm(cy + r * std::sin(a)));
    }
    return out;
}

Polygon star(Gen &g, double cx, double cy, double r_in, double r_out, int n)
{
    Polygon out;
    for (int i = 0; i < 2 * n; ++i) {
        const double a = PI * double(i) / double(n);
        const double r = (i & 1) ? g.real(r_in * 0.6, r_in) : g.real(r_in * 1.2, r_out);
        out.points.emplace_back(mm(cx + r * std::cos(a)), mm(cy + r * std::sin(a)));
    }
    return out;
}

ExPolygon with_holes(Polygon contour, Polygons holes)
{
    if (!contour.is_counter_clockwise())
        contour.reverse();
    for (Polygon &h : holes)
        if (h.is_counter_clockwise())
            h.reverse();
    ExPolygon e(std::move(contour));
    e.holes = std::move(holes);
    return e;
}

namespace {

Polygon pts(std::initializer_list<std::pair<double, double>> list)
{
    Polygon out;
    for (const auto &q : list)
        out.points.emplace_back(mm(q.first), mm(q.second));
    return out;
}

} // namespace

std::vector<Shape> hand_shapes()
{
    std::vector<Shape> out;
    Gen                g(0xF111AB1Eull);
    out.push_back({"square", with_holes(rect(0, 0, 8, 8), {})});
    out.push_back({"rectangle", with_holes(rect(-3, 1, 14, 5), {})});
    out.push_back({"square with a hole", with_holes(rect(0, 0, 10, 10), {rect(3, 3, 4, 4)})});
    out.push_back({"L", with_holes(pts({{0, 0}, {10, 0}, {10, 3}, {3, 3}, {3, 10}, {0, 10}}), {})});
    out.push_back({"comb", with_holes(pts({{0, 0}, {12, 0}, {12, 8}, {10.5, 8}, {10.5, 2}, {9, 2}, {9, 8}, {7.5, 8}, {7.5, 2}, {6, 2}, {6, 8}, {4.5, 8}, {4.5, 2}, {3, 2}, {3, 8}, {1.5, 8}, {1.5, 2}, {0, 2}}), {})});
    out.push_back({"thin strip", with_holes(rect(0, 0, 12, 0.9), {})});
    out.push_back({"disc", with_holes(ngon(0, 0, 5, 64, 0.), {})});
    out.push_back({"annulus", with_holes(ngon(0, 0, 6, 72, 0.), {ngon(0, 0, 3, 48, 0.1)})});
    out.push_back({"star", with_holes(star(g, 0, 0, 2, 6, 7), {})});
    out.push_back({"dumbbell", with_holes(pts({{0, 0}, {5, 0}, {5, 2.2}, {9, 2.2}, {9, 0}, {14, 0}, {14, 5}, {9, 5}, {9, 2.8}, {5, 2.8}, {5, 5}, {0, 5}}), {})});
    out.push_back({"speck", with_holes(rect(0, 0, 0.3, 0.3), {})});
    out.push_back({"square with two holes", with_holes(rect(0, 0, 12, 8), {rect(2, 2, 3, 4), ngon(8.5, 4, 1.5, 24, 0.)})});
    out.push_back({"turned square", with_holes(pts({{4, 0}, {8, 4}, {4, 8}, {0, 4}}), {})});
    out.push_back({"triangle", with_holes(pts({{0, 0}, {11, 0}, {3, 7}}), {})});
    // OrcaSlicer's tests/fff_print/test_fill.cpp: "Diamond with endpoints on grid" at a tenth of its
    // size, and the diamond and the cut contour of its later sections, as they are (scaled).
    out.push_back({"test_fill grid diamond", with_holes(pts({{0, 0}, {10, 0}, {15, 5}, {10, 10}, {0, 10}, {-5, 5}}), {})});
    out.push_back({"test_fill diamond", with_holes(Polygon({Point(25771516, 14142125), Point(14142138, 25771515), Point(2512749, 14142131), Point(14142125, 2512749)}), {})});
    out.push_back({"test_fill cut", with_holes(Polygon({Point(59515297, 5422499), Point(59531249, 5578697), Point(59695801, 6123186), Point(59965713, 6630228), Point(60328214, 7070685), Point(60773285, 7434379), Point(61274561, 7702115), Point(61819378, 7866770), Point(62390306, 7924789), Point(62958700, 7866744), Point(63503012, 7702244), Point(64007365, 7434357), Point(64449960, 7070398), Point(64809327, 6634999), Point(65082143, 6123325), Point(65245005, 5584454), Point(65266967, 5422499), Point(66267307, 5422499), Point(66269190, 8310081), Point(66275379, 17810072), Point(66277259, 20697500), Point(65267237, 20697500), Point(65245004, 20533538), Point(65082082, 19994444), Point(64811462, 19488579), Point(64450624, 19048208), Point(64012101, 18686514), Point(63503122, 18415781), Point(62959151, 18251378), Point(62453416, 18198442), Point(62390147, 18197355), Point(62200087, 18200576), Point(61813519, 18252990), Point(61274433, 18415918), Point(60768598, 18686517), Point(60327567, 19047892), Point(59963609, 19493297), Point(59695865, 19994587), Point(59531222, 20539379), Point(59515153, 20697500), Point(58502480, 20697500), Point(58502480, 5422499)}), {})});
    return out;
}

ExPolygon half(const ExPolygon &e)
{
    ExPolygon out = e;
    out.scale(0.5);
    return out;
}

ExPolygon random_shape(Gen &g, double size_mm)
{
    if (g.one_in(3)) {
        const double r = size_mm * 0.5;
        Polygons     holes;
        if (g.one_in(2))
            holes.push_back(ngon(g.real(-0.15, 0.15) * r, g.real(-0.15, 0.15) * r, r * g.real(0.15, 0.3), int(g.range(5, 24)), g.real(0, 1)));
        return with_holes(star(g, 0, 0, r * 0.55, r, int(g.range(3, 12))), std::move(holes));
    }
    static const std::vector<Shape> shapes = hand_shapes();
    // The hand shapes but the speck and the three of test_fill.cpp at their own size.
    ExPolygon        e     = shapes[size_t(g.below(shapes.size() - 4))].e;
    const BoundingBox bb   = get_extents(e.contour);
    const double     span  = double(std::max(bb.size().x(), bb.size().y()));
    const double     f     = span > 0 ? double(mm(size_mm)) / span : 1.;
    e.translate(-bb.center().x(), -bb.center().y());
    e.scale(f);
    e.rotate(g.real(-PI, PI));
    e.translate(Point(mm(g.real(-50, 50)), mm(g.real(-50, 50))));
    return e;
}

bool mesh_sections(std::vector<Shape> &out, std::string &error)
{
    char *pgm = nullptr;
    if (_get_pgmptr(&pgm) != 0 || pgm == nullptr) {
        error = "cannot find the probe's own path";
        return false;
    }
    const std::filesystem::path data = std::filesystem::path(pgm).parent_path().parent_path() / "src" / "tests" / "data";
    static const char *const meshes[] = {"cube_with_concave_hole.obj", "two_hollow_squares.obj", "A.obj", "frog_legs.obj", "bridge.obj", "ipadstand.obj", "extruder_idler.obj"};
    static const double      heights[] = {0.3, 0.55, 0.8};
    for (const char *name : meshes) {
        TriangleMesh mesh;
        ObjInfo      info;
        std::string  message;
        const std::string path = (data / name).string();
        if (!load_obj(path.c_str(), &mesh, info, message)) {
            error = "cannot read " + path + ": " + message;
            return false;
        }
        const BoundingBoxf3 bb = mesh.bounding_box();
        std::vector<float>  zs;
        for (double h : heights)
            zs.push_back(float(bb.min.z() + (bb.max.z() - bb.min.z()) * h));
        const std::vector<ExPolygons> slices = slice_mesh_ex(mesh.its, zs);
        for (size_t i = 0; i < slices.size(); ++i)
            for (size_t j = 0; j < slices[i].size(); ++j)
                out.push_back({std::string(name) + " at " + std::to_string(int(heights[i] * 100)) + "% #" + std::to_string(j), slices[i][j]});
    }
    return true;
}

// ---------------------------------------------------------------- the committed set

void Recorder::commit(const Payload &p)
{
    sets_.committed.add(kind_, p);
    bytes_ += p.bytes.size() + 12;
    ++committed_;
}

void Recorder::edge(const Payload &p)
{
    sets_.full.add(kind_, p);
    ++full_;
    if (fits(p))
        commit(p);
}

void Recorder::seeded(const Payload &p)
{
    sets_.full.add(kind_, p);
    ++full_;
    if (taken_ < cap_ && p.bytes.size() <= max_bytes_ && fits(p)) {
        commit(p);
        ++taken_;
    }
}

// ---------------------------------------------------------------- what a call prints

ErrorLog::ErrorLog()
{
    namespace logging = boost::log;
    namespace sinks   = boost::log::sinks;
    namespace expr    = boost::log::expressions;
    auto backend      = boost::make_shared<sinks::text_ostream_backend>();
    backend->add_stream(boost::shared_ptr<std::ostream>(&text_, boost::null_deleter()));
    backend->auto_flush(true);
    sink_ = boost::make_shared<sinks::synchronous_sink<sinks::text_ostream_backend>>(backend);
    sink_->set_filter(logging::trivial::severity >= logging::trivial::error);
    sink_->set_formatter(expr::stream << "[" << logging::trivial::severity << "] " << expr::smessage);
    logging::core::get()->add_sink(sink_);
}

ErrorLog::~ErrorLog() { boost::log::core::get()->remove_sink(sink_); }

std::string ErrorLog::take()
{
    sink_->flush();
    std::string out = text_.str();
    text_.str(std::string());
    text_.clear();
    return out;
}

namespace {

ErrorLog *g_log = nullptr;

} // namespace

ErrorLogScope::ErrorLogScope() { g_log = &log_; }
ErrorLogScope::~ErrorLogScope() { g_log = nullptr; }

std::string take_errors() { return g_log ? g_log->take() : std::string(); }

bool capture_stdout(const std::function<void()> &f, std::string &out, std::string &error)
{
    std::fflush(stdout);
    const int             fd = _fileno(stdout);
    std::error_code       ec;
    std::filesystem::path tmp = std::filesystem::temp_directory_path(ec);
    if (ec) {
        error = "no temporary folder for stdout";
        return false;
    }
    tmp /= "sketch-orca-probe-fill-" + std::to_string(_getpid()) + ".out";
    FILE *t = nullptr;
    if (fd < 0 || _wfopen_s(&t, tmp.c_str(), L"w+b") != 0 || t == nullptr) {
        error = "cannot send stdout to " + tmp.string();
        return false;
    }
    const int saved = _dup(fd);
    if (saved < 0 || _dup2(_fileno(t), fd) != 0) {
        std::fclose(t);
        std::filesystem::remove(tmp, ec);
        error = "cannot move stdout";
        return false;
    }
    f();
    std::fflush(stdout);
    _dup2(saved, fd);
    _close(saved);
    std::fseek(t, 0, SEEK_END);
    const long size = std::ftell(t);
    out.assign(size_t(size < 0 ? 0 : size), '\0');
    std::fseek(t, 0, SEEK_SET);
    const bool read = out.empty() || std::fread(&out[0], 1, out.size(), t) == out.size();
    std::fclose(t);
    std::filesystem::remove(tmp, ec);
    if (!read) {
        error = "cannot read " + tmp.string();
        return false;
    }
    return true;
}

// ---------------------------------------------------------------- lines

Polylines clipped_lines(const ExPolygon &e, double angle, double spacing_mm, double shift)
{
    ExPolygon r = e;
    r.rotate(-angle);
    const BoundingBox bb = get_extents(r.contour);
    coord_t           s  = coord_t(mm(spacing_mm));
    if (s < 1)
        s = 1;
    Polylines lines;
    for (coord_t x = bb.min.x() + coord_t(shift * double(s)); x <= bb.max.x(); x += s) {
        Polyline pl(Point(x, bb.min.y() - s), Point(x, bb.max.y() + s));
        pl.rotate(angle);
        lines.push_back(std::move(pl));
    }
    return intersection_pl(lines, to_polygons(e));
}

} // namespace fill
} // namespace probe
