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
// walls.sop1's values (values.hpp): the writers of parity/SOP1.md's compound values and the
// probe's own, the configurations made from settings, and the shapes. An entity is written by its
// exact type, as parity/SOP1.md's entity table gives the kinds (orca-o3-probe-walls, 2026-11-13).
#include "values.hpp"

#include <cmath>
#include <typeinfo>

namespace probe::walls {

using namespace Slic3r;

void put_point(Payload &p, const Point &pt) { p.i64(int64_t(pt.x())).i64(int64_t(pt.y())); }

void put_points(Payload &p, const Points &pts)
{
    p.u64(pts.size());
    for (const Point &pt : pts)
        put_point(p, pt);
}

void put_polygon(Payload &p, const Polygon &polygon) { put_points(p, polygon.points); }

void put_polygons(Payload &p, const Polygons &polygons)
{
    p.u64(polygons.size());
    for (const Polygon &polygon : polygons)
        put_polygon(p, polygon);
}

void put_expolygon(Payload &p, const ExPolygon &expolygon)
{
    put_polygon(p, expolygon.contour);
    put_polygons(p, expolygon.holes);
}

void put_expolygons(Payload &p, const ExPolygons &expolygons)
{
    p.u64(expolygons.size());
    for (const ExPolygon &e : expolygons)
        put_expolygon(p, e);
}

void put_surface(Payload &p, const Surface &s)
{
    p.u32(uint32_t(s.surface_type));
    put_expolygon(p, s.expolygon);
    p.f64(s.thickness);
    p.u16(uint16_t(s.thickness_layers));
    p.f64(s.bridge_angle);
    p.u16(uint16_t(s.extra_perimeters));
}

void put_surfaces(Payload &p, const Surfaces &surfaces)
{
    p.u64(surfaces.size());
    for (const Surface &s : surfaces)
        put_surface(p, s);
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
        put_point(p, arc.center);
        p.f64(arc.radius);
        p.f64(arc.length);
        p.f64(arc.angle_radians);
        p.f64(arc.polar_start_theta);
        p.f64(arc.polar_end_theta);
        put_point(p, arc.start_point);
        put_point(p, arc.end_point);
        p.u8(uint8_t(arc.direction));
    }
}

void put_path_fields(Payload &p, const ExtrusionPath &path)
{
    p.u8(uint8_t(path.role()));
    p.i32(int32_t(path.inset_idx));
    p.u64(path.polyline.points.size());
    for (const Point3 &pt : path.polyline.points)
        p.i64(int64_t(pt.x())).i64(int64_t(pt.y())).i64(int64_t(pt.z()));
    put_fitting(p, path.polyline.fitting_result);
    p.f64(path.overhang_degree);
    p.i32(int32_t(path.curve_degree));
    p.f64(path.mm3_per_mm);
    p.f32(path.width);
    p.f32(path.height);
    p.f64(path.smooth_speed);
    p.boolean(path.z_contoured);
    p.boolean(path.is_force_no_extrusion());
    p.boolean(path.can_reverse());
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

void put_polyline(Payload &p, const Polyline &polyline)
{
    put_points(p, polyline.points);
    put_fitting(p, polyline.fitting_result);
}

void put_polylines(Payload &p, const Polylines &polylines)
{
    p.u64(polylines.size());
    for (const Polyline &pl : polylines)
        put_polyline(p, pl);
}

void put_entity(Payload &p, const ExtrusionEntity &entity)
{
    const std::type_info &type = typeid(entity);
    if (type == typeid(ExtrusionPath)) {
        p.u8(1);
        put_path_fields(p, static_cast<const ExtrusionPath &>(entity));
    } else if (type == typeid(ExtrusionPathContoured)) {
        const auto &path = static_cast<const ExtrusionPathContoured &>(entity);
        p.u8(2);
        put_path_fields(p, path);
        p.u64(path.z_diffs.size());
        for (double z : path.z_diffs)
            p.f64(z);
    } else if (type == typeid(ExtrusionPathSloped)) {
        const auto &path = static_cast<const ExtrusionPathSloped &>(entity);
        p.u8(3);
        put_path_fields(p, path);
        p.f64(path.slope_begin.z_ratio).f64(path.slope_begin.e_ratio).f64(path.slope_end.z_ratio).f64(path.slope_end.e_ratio);
    } else if (type == typeid(ExtrusionPathOriented)) {
        p.u8(4);
        put_path_fields(p, static_cast<const ExtrusionPath &>(entity));
    } else if (type == typeid(ExtrusionMultiPath)) {
        const auto &multi = static_cast<const ExtrusionMultiPath &>(entity);
        p.u8(5);
        p.i32(int32_t(multi.inset_idx));
        p.boolean(multi.can_reverse());
        p.u64(multi.paths.size());
        for (const ExtrusionPath &path : multi.paths)
            put_entity(p, path);
    } else if (type == typeid(ExtrusionLoop)) {
        p.u8(6);
        put_loop_fields(p, static_cast<const ExtrusionLoop &>(entity));
    } else if (type == typeid(ExtrusionLoopSloped)) {
        const auto &loop = static_cast<const ExtrusionLoopSloped &>(entity);
        p.u8(7);
        put_loop_fields(p, loop);
        p.u64(loop.starts.size());
        for (const ExtrusionPathSloped &path : loop.starts)
            put_entity(p, path);
        p.u64(loop.ends.size());
        for (const ExtrusionPathSloped &path : loop.ends)
            put_entity(p, path);
    } else if (type == typeid(ExtrusionEntityCollection)) {
        const auto &c = static_cast<const ExtrusionEntityCollection &>(entity);
        p.u8(8);
        p.i32(int32_t(c.inset_idx));
        p.boolean(c.no_sort);
        p.boolean(c.can_reverse());
        p.u64(c.entities.size());
        for (const ExtrusionEntity *child : c.entities)
            put_entity(p, *child);
    } else {
        p.u8(0);
        p.str(type.name());
    }
}

void put_paths(Payload &p, const ExtrusionPaths &paths)
{
    p.u64(paths.size());
    for (const ExtrusionPath &path : paths)
        put_entity(p, path);
}

void put_flow(Payload &p, const Flow &flow) { p.f32(flow.width()).f32(flow.height()).f32(flow.spacing()).f32(flow.nozzle_diameter()).boolean(flow.bridge()); }

void put_settings(Payload &p, const Settings &settings)
{
    p.u64(settings.size());
    for (const auto &kv : settings)
        p.str(kv.first).str(kv.second);
}

void put_lines(Payload &p, const Lines &lines)
{
    p.u64(lines.size());
    for (const Line &l : lines) {
        put_point(p, l.a);
        put_point(p, l.b);
    }
}

void put_zpath(Payload &p, const ClipperLib_Z::Path &path)
{
    p.u64(path.size());
    for (const ClipperLib_Z::IntPoint &pt : path)
        p.i64(int64_t(pt.x())).i64(int64_t(pt.y())).i64(int64_t(pt.z()));
}

void put_zpaths(Payload &p, const ClipperLib_Z::Paths &paths)
{
    p.u64(paths.size());
    for (const ClipperLib_Z::Path &path : paths)
        put_zpath(p, path);
}

void put_arachne_line(Payload &p, const Arachne::ExtrusionLine &line)
{
    p.u64(uint64_t(line.inset_idx)).boolean(line.is_odd).boolean(line.is_closed);
    p.u64(line.junctions.size());
    for (const Arachne::ExtrusionJunction &j : line.junctions)
        p.i64(int64_t(j.p.x())).i64(int64_t(j.p.y())).i64(int64_t(j.w)).u64(uint64_t(j.perimeter_index));
}

void make_configs(const Settings &settings, Configs &out)
{
    DynamicPrintConfig dyn;
    for (const auto &kv : settings)
        dyn.set_deserialize_strict(kv.first, kv.second);
    out.region.apply(dyn, true);
    out.object.apply(dyn, true);
    out.print.apply(dyn, true);
}

namespace {

coord_t sc(double mm) { return coord_t(std::llround(mm * 1000000.)); }

} // namespace

Polygon rect(double x0, double y0, double x1, double y1)
{
    Polygon p;
    p.points = {Point(sc(x0), sc(y0)), Point(sc(x1), sc(y0)), Point(sc(x1), sc(y1)), Point(sc(x0), sc(y1))};
    return p;
}

Polygon circle(double cx, double cy, double r, int n, double phase)
{
    Polygon p;
    for (int i = 0; i < n; ++i) {
        const double a = phase + 2. * 3.14159265358979323846 * double(i) / double(n);
        p.points.emplace_back(sc(cx + r * std::cos(a)), sc(cy + r * std::sin(a)));
    }
    return p;
}

Polygon poly(std::initializer_list<std::pair<double, double>> mm)
{
    Polygon p;
    for (const auto &xy : mm)
        p.points.emplace_back(sc(xy.first), sc(xy.second));
    return p;
}

ExPolygon with_holes(Polygon contour, std::vector<Polygon> holes)
{
    ExPolygon e;
    contour.make_counter_clockwise();
    e.contour = std::move(contour);
    for (Polygon &h : holes) {
        h.make_clockwise();
        e.holes.push_back(std::move(h));
    }
    return e;
}

Polygon rotated(Polygon p, double angle, double cx, double cy)
{
    const double c = std::cos(angle), s = std::sin(angle);
    for (Point &pt : p.points) {
        const double x = double(pt.x()) - double(sc(cx)), y = double(pt.y()) - double(sc(cy));
        pt = Point(coord_t(std::llround(c * x - s * y)) + sc(cx), coord_t(std::llround(s * x + c * y)) + sc(cy));
    }
    return p;
}

ExPolygon rotated(const ExPolygon &e, double angle, double cx, double cy)
{
    ExPolygon out;
    out.contour = rotated(e.contour, angle, cx, cy);
    for (const Polygon &h : e.holes)
        out.holes.push_back(rotated(h, angle, cx, cy));
    return out;
}

double real(SplitMix &g, double a, double b) { return a + (b - a) * (double(g.next() >> 11) * 0x1.0p-53); }

} // namespace probe::walls
