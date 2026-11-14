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
// The voronoi probe's records of voronoi-4 (parity/sop1/voronoi.md, VRGD to VRMA): the repair that
// Slic3r::Geometry::VoronoiDiagram::construct_voronoi runs on segment input (detection, the
// rotations it ran and what each found, the stale diagram a second construction after a failed
// repair reads, the logs) on the shapes, test_voronoi.cpp's line sets and the repair search's draws,
// the two planarity checks of VoronoiUtilsCgal, Voronoi::annotate_inside_outside (VoronoiOffset.cpp,
// which MedialAxis calls) and the medial axis, through MedialAxis::build and ExPolygon::medial_axis
// on the thin-wall and gap-fill regions the walls and the fill give it. Written by
// orca-o3-probe-voronoi, 2026-11-13.
#include "common.hpp"

#include <array>
#include <map>
#include <utility>

#include "libslic3r/Arachne/utils/PolygonsSegmentIndex.hpp"
#include "libslic3r/ClipperUtils.hpp"
#include "libslic3r/Geometry/MedialAxis.hpp"
#include "libslic3r/Geometry/Voronoi.hpp"
#include "libslic3r/Geometry/VoronoiOffset.hpp"
#include "libslic3r/Geometry/VoronoiUtilsCgal.hpp"

namespace probe::vr {

namespace {

using namespace Slic3r;
using Slic3r::Geometry::VoronoiDiagram;
using Slic3r::Geometry::VoronoiUtilsCgal;
using PSI = Slic3r::Arachne::PolygonsSegmentIndex;

std::vector<PSI> psi_of(const Polygons &polys)
{
    std::vector<PSI> segs;
    for (unsigned poly = 0; poly < polys.size(); ++poly)
        for (unsigned point = 0; point < polys[poly].size(); ++point)
            segs.emplace_back(&polys, poly, point);
    return segs;
}

void put_state(Payload &p, const VoronoiDiagram &vd)
{
    p.u8(uint8_t(vd.get_issue_type())).u8(uint8_t(vd.get_state())).boolean(vd.is_valid());
    p.u64(vd.num_vertices()).u64(vd.num_edges()).u64(vd.num_cells());
}

// ---------------------------------------------------------------- VRGD

using State     = VoronoiDiagram::State;
using IssueType = VoronoiDiagram::IssueType;

// Whether construct_voronoi's repair ran (and replaced the builder's diagram with a rotation's).
bool repaired(const VoronoiDiagram &vd) { return vd.get_state() == State::REPAIR_SUCCESSFUL || vd.get_state() == State::REPAIR_UNSUCCESSFUL; }

// The angles of try_to_repair_degenerated_voronoi_diagram, in its order.
const double fix_angles[] = {PI / 6, PI / 5, PI / 7, PI / 11};

// The rotations construct_voronoi's repair ran on these segments, each the issue its detection
// found: none when the repair did not run, else the angles in turn until one finds no issue. Each
// is the public try_to_repair_degenerated_voronoi_diagram_by_rotation on a fresh object, and the
// last one's diagram must be the one construct_voronoi left (else `error`).
template<typename It> std::vector<uint8_t> rotations(It begin, It end, const VoronoiDiagram &vd, std::string &error)
{
    std::vector<uint8_t> out;
    if (!repaired(vd))
        return out;
    VoronoiDiagram scratch;
    for (const double a : fix_angles) {
        const IssueType t = scratch.try_to_repair_degenerated_voronoi_diagram_by_rotation(begin, end, a);
        out.push_back(uint8_t(t));
        if (t == IssueType::NO_ISSUE_DETECTED)
            break;
    }
    Payload mine, theirs;
    put_diagram(mine, scratch);
    put_diagram(theirs, vd);
    if (error.empty() && (mine.bytes != theirs.bytes || out.back() != uint8_t(vd.get_issue_type())))
        error = "voronoi: the rotations of a VRGD record do not rebuild the diagram construct_voronoi repaired";
    return out;
}

void put_rotations(Payload &p, const std::vector<uint8_t> &rot)
{
    p.u8(uint8_t(rot.size()));
    for (const uint8_t t : rot)
        p.u8(t);
}

// The issue construct_voronoi's detection found on the builder's diagram, from the warning it
// writes before the repair (0 when it wrote none).
uint8_t detected_issue(const std::string &log)
{
    static const std::pair<const char *, uint8_t> warnings[] = {
        {"Detected finite Voronoi vertex with non finite vertex, input", 1},     {"Detected missing Voronoi vertex, input", 2},
        {"Detected non-planar Voronoi diagram, input", 3},                         {"Detected Voronoi edge intersecting input segment, input", 4},
        {"Detected parabolic Voronoi edges without focus point, input", 5},        {"Detected unknown Voronoi diagram issue, input", 6}};
    for (const auto &[text, issue] : warnings)
        if (log.find(text) != std::string::npos)
            return issue;
    return 0;
}

// What a VRGD record of lines starts with: its source (0 a shape's lines, 2 a line set, 3 the
// search's lines) and, for the search's, the draw, its family and its turn (0 as drawn, k the draw
// turned by the k-th angle of the repair, as the repair turns it).
struct Head
{
    uint8_t  source;
    uint32_t draw;
    uint8_t  family;
    uint8_t  turn;
};

// What a VRGD record of lines shows: the issue the detection found (from its warning), the
// rotations run and what each found, the issue and the state left, and the second construction
// (0 none, 1 returned with second_state, 2 threw).
struct Outcome
{
    uint8_t                detected = 0;
    std::vector<uint8_t>   rotations;
    uint8_t                issue = 0, state = 0, second = 0, second_state = 0;
    std::array<uint8_t, 6> key() const { return {detected, uint8_t(rotations.size()), issue, state, second, second_state}; }
};

// One VRGD record of lines. `ex` is the ExPolygon the lines come from when there is one:
// after a failed repair MedialAxis::build constructs again, on the same VoronoiDiagram, from the
// lines of its morphological closing. That detection reads the stale diagram of the failed repair
// with the new lines, so a lookup there can throw out of construct_voronoi: u8 0 then the state
// and the diagram, or u8 1 then the error.
Payload vrgd_lines(const Head &h, Lines lines, bool repair, const ExPolygon *ex, Outcome &o, std::string &error)
{
    VoronoiDiagram vd;
    vd.construct_voronoi(lines.begin(), lines.end(), repair);
    const std::string log = take_log();
    Payload           p;
    p.u8(h.source);
    if (h.source == 3)
        p.u32(h.draw).u8(h.family).u8(h.turn);
    p.boolean(repair);
    put_lines(p, lines);
    put_state(p, vd);
    o.detected = detected_issue(log);
    o.issue    = uint8_t(vd.get_issue_type());
    o.state    = uint8_t(vd.get_state());
    if (repair) {
        o.rotations = rotations(lines.begin(), lines.end(), vd, error);
        put_rotations(p, o.rotations);
    }
    // The diagram when the repair replaced the builder's; a line set's builder diagram too, which no
    // VRVD record holds.
    const bool has = repaired(vd) || (h.source == 2 && !repair);
    p.boolean(has);
    if (has)
        put_diagram(p, vd);
    p.str(log + take_log());
    const bool second = repair && !vd.is_valid() && ex != nullptr;
    p.boolean(second);
    if (second) {
        Lines again = to_lines(closing_ex({*ex}, float(2. * SCALED_EPSILON)));
        put_lines(p, again);
        try {
            vd.construct_voronoi(again.begin(), again.end());
            p.u8(0);
            put_state(p, vd);
            put_diagram(p, vd);
            o.second       = 1;
            o.second_state = uint8_t(vd.get_state());
        } catch (const std::exception &e) {
            p.u8(1);
            put_error(p, e);
            o.second = 2;
        }
        p.str(take_log());
    }
    return p;
}

// The search (search.cpp): every draw whose detection finds an issue, and after it, for each
// rotation of its repair whose detection found an issue other than a missing vertex, the draw turned
// by that rotation's angle with Point::rotated, as the repair turns it. That turned input gives the
// builder the rotation's segments, so its own detection must find what the rotation found (else
// `error`). Of each outcome (Outcome::key) the smallest record is committed.
void record_search(Sets &sets, Budget &b, std::string &error)
{
    struct Kept
    {
        Payload                p;
        std::array<uint8_t, 6> key;
    };
    std::vector<Kept> kept;
    for (uint32_t draw = 0; draw < kSearchCount; ++draw) {
        const SearchShape sh = search_shape(draw);
        if (sh.ex.empty())
            continue;
        {
            Lines          lines = to_lines(sh.ex);
            VoronoiDiagram vd;
            vd.construct_voronoi(lines.begin(), lines.end(), true);
            take_log();
            if (!repaired(vd))
                continue;
        }
        Outcome o;
        Payload p = vrgd_lines({3, draw, sh.family, 0}, to_lines(sh.ex), true, &sh.ex.front(), o, error);
        kept.push_back({std::move(p), o.key()});
        for (size_t k = 0; k < o.rotations.size(); ++k) {
            const uint8_t found = o.rotations[k];
            if (found == uint8_t(IssueType::NO_ISSUE_DETECTED) || found == uint8_t(IssueType::MISSING_VORONOI_VERTEX))
                continue;
            ExPolygon turned = sh.ex.front();
            for (Point &q : turned.contour.points)
                q = q.rotated(fix_angles[k]);
            for (Polygon &hole : turned.holes)
                for (Point &q : hole.points)
                    q = q.rotated(fix_angles[k]);
            Outcome ot;
            Payload pt = vrgd_lines({3, draw, sh.family, uint8_t(k + 1)}, to_lines(turned), true, &turned, ot, error);
            if (ot.detected != found && error.empty())
                error = "voronoi: the detection on a turned draw differs from the detection of its rotation";
            kept.push_back({std::move(pt), ot.key()});
        }
    }
    std::map<std::array<uint8_t, 6>, size_t> smallest;
    for (size_t i = 0; i < kept.size(); ++i) {
        const auto it = smallest.find(kept[i].key);
        if (it == smallest.end() || kept[i].p.bytes.size() < kept[it->second].p.bytes.size())
            smallest[kept[i].key] = i;
    }
    for (size_t i = 0; i < kept.size(); ++i)
        put_record(sets, "VRGD", kept[i].p, smallest[kept[i].key] == i && b.take(kept[i].p, true));
}

void record_repairs(Sets &sets, std::string &error)
{
    Budget b{4, 9000};
    take_log();
    for (const Shape &s : shapes()) {
        {
            const Lines lines = to_lines(s.ex);
            if (!lines.empty() && lines.size() <= 800) {
                // A hand-made shape's record is committed as hand-made when it holds a diagram or the
                // repair is off.
                for (int repair = 1; repair >= 0; --repair) {
                    Outcome       o;
                    const Payload p   = vrgd_lines({0, 0, 0, 0}, lines, repair != 0, s.ex.size() == 1 ? &s.ex.front() : nullptr, o, error);
                    const bool    has = o.state == uint8_t(State::REPAIR_SUCCESSFUL) || o.state == uint8_t(State::REPAIR_UNSUCCESSFUL);
                    put_record(sets, "VRGD", p, b.take(p, s.hand && (has || repair == 0)));
                }
            }
        }
        {
            const Polygons polys = arachne_polygons(s.ex);
            const size_t   n     = segment_count(polys);
            if (n != 0 && n <= 800) {
                const std::vector<PSI> segs = psi_of(polys);
                VoronoiDiagram         vd;
                vd.construct_voronoi(segs.cbegin(), segs.cend());
                const std::string log = take_log();
                Payload           p;
                p.u8(1).boolean(true);
                put_polys(p, polys);
                put_state(p, vd);
                put_rotations(p, rotations(segs.cbegin(), segs.cend(), vd, error));
                const bool has = repaired(vd);
                p.boolean(has);
                if (has)
                    put_diagram(p, vd);
                p.str(log + take_log());
                p.boolean(false);
                put_record(sets, "VRGD", p, b.take(p, s.hand));
            }
        }
    }
    // test_voronoi.cpp's line sets: with the repair on (committed when the set is hand-made and fits),
    // and in the full set also off.
    for (const LineSet &ls : issue_line_sets())
        for (int repair = 1; repair >= 0; --repair) {
            Outcome       o;
            const Payload p = vrgd_lines({2, 0, 0, 0}, ls.lines, repair != 0, nullptr, o, error);
            put_record(sets, "VRGD", p, repair != 0 && ls.hand && b.take(p, true));
        }
    record_search(sets, b, error);
}

// ---------------------------------------------------------------- VRCG

// u8 0 false, 1 true, 2 threw (then the error).
template<typename F> void put_check(Payload &p, F f)
{
    try {
        p.u8(f() ? 1 : 0);
    } catch (const std::exception &e) {
        p.u8(2);
        put_error(p, e);
    }
}

void record_planarity(Sets &sets)
{
    Budget b{40, 1500};
    for (const Shape &s : shapes()) {
        {
            Lines lines = to_lines(s.ex);
            if (!lines.empty() && lines.size() <= 800) {
                VoronoiDiagram vd;
                vd.construct_voronoi(lines.begin(), lines.end(), false);
                Payload p;
                p.u8(0);
                put_lines(p, lines);
                put_check(p, [&] { return VoronoiUtilsCgal::is_voronoi_diagram_planar_angle(vd, lines.begin(), lines.end()); });
                put_check(p, [&] { return VoronoiUtilsCgal::is_voronoi_diagram_planar_intersection(vd); });
                put_record(sets, "VRCG", p, b.take(p, s.hand));
            }
        }
        {
            const Polygons polys = arachne_polygons(s.ex);
            const size_t   n     = segment_count(polys);
            if (n != 0 && n <= 800) {
                const std::vector<PSI> segs = psi_of(polys);
                VoronoiDiagram         vd;
                vd.construct_voronoi(segs.cbegin(), segs.cend(), false);
                Payload p;
                p.u8(1);
                put_polys(p, polys);
                put_check(p, [&] { return VoronoiUtilsCgal::is_voronoi_diagram_planar_angle(vd, segs.cbegin(), segs.cend()); });
                put_check(p, [&] { return VoronoiUtilsCgal::is_voronoi_diagram_planar_intersection(vd); });
                put_record(sets, "VRCG", p, b.take(p, s.hand));
            }
        }
    }
    // Diagrams of points: no parabolic edges, so the angle check reads no segment.
    for (const SiteSet &ss : site_sets()) {
        if (ss.points.size() < 3 || !ss.segments.empty() || ss.points.size() > 60)
            continue;
        Points pts;
        for (const Pt32 &q : ss.points)
            pts.emplace_back(q.x, q.y);
        VoronoiDiagram vd;
        vd.construct_voronoi(pts.begin(), pts.end());
        Lines   none;
        Payload p;
        p.u8(2);
        put_pts(p, pts);
        put_check(p, [&] { return VoronoiUtilsCgal::is_voronoi_diagram_planar_angle(vd, none.begin(), none.end()); });
        put_check(p, [&] { return VoronoiUtilsCgal::is_voronoi_diagram_planar_intersection(vd); });
        put_record(sets, "VRCG", p, b.take(p, ss.hand));
    }
}

// ---------------------------------------------------------------- VRAN

void record_annotations(Sets &sets)
{
    Budget b{6, 3000};
    take_log();
    for (const Shape &s : shapes()) {
        Lines lines = to_lines(s.ex);
        if (lines.empty() || lines.size() > 2000)
            continue;
        VoronoiDiagram vd;
        vd.construct_voronoi(lines.begin(), lines.end());
        Slic3r::Voronoi::annotate_inside_outside(vd, lines);
        Payload p;
        put_lines(p, lines);
        put_state(p, vd);
        p.u32(uint32_t(vd.cells().size()));
        for (const auto &c : vd.cells())
            p.u8(uint8_t(c.color()));
        p.u32(uint32_t(vd.vertices().size()));
        for (const auto &v : vd.vertices())
            p.u8(uint8_t(v.color()));
        p.u32(uint32_t(vd.edges().size()));
        for (const auto &e : vd.edges())
            p.u8(uint8_t(e.color()));
        p.str(take_log());
        put_record(sets, "VRAN", p, b.take(p, s.hand));
    }
}

// ---------------------------------------------------------------- VRMA

void put_thick(Payload &p, const ThickPolylines &tp)
{
    p.u32(uint32_t(tp.size()));
    for (const ThickPolyline &t : tp) {
        put_pts(p, t.points);
        p.u32(uint32_t(t.width.size()));
        for (double w : t.width)
            p.f64(w);
        p.boolean(t.endpoints.first).boolean(t.endpoints.second);
    }
}

bool same_points(const Polylines &a, const ThickPolylines &b)
{
    if (a.size() != b.size())
        return false;
    for (size_t i = 0; i < a.size(); ++i)
        if (a[i].points != b[i].points)
            return false;
    return true;
}

void record_medial_axis(Sets &sets)
{
    Budget b{60, 3000};
    take_log();
    for (const ThinCase &t : thin_cases()) {
        Payload p;
        put_expoly(p, t.ex);
        p.f64(t.min_width).f64(t.max_width);
        try {
            ThickPolylines build;
            {
                Slic3r::Geometry::MedialAxis ma(t.min_width, t.max_width, t.ex);
                ma.build(&build);
            }
            Polylines build_lines;
            {
                Slic3r::Geometry::MedialAxis ma(t.min_width, t.max_width, t.ex);
                ma.build(&build_lines);
            }
            ThickPolylines medial;
            t.ex.medial_axis(t.min_width, t.max_width, &medial);
            Polylines medial_lines;
            t.ex.medial_axis(t.min_width, t.max_width, &medial_lines);
            p.u8(0);
            put_thick(p, build);
            put_thick(p, medial);
            p.boolean(same_points(build_lines, build)).boolean(same_points(medial_lines, medial));
        } catch (const std::exception &e) {
            p.u8(1);
            put_error(p, e);
        }
        p.str(take_log());
        put_record(sets, "VRMA", p, b.take(p, t.hand));
    }
}

} // namespace

bool record_slic3r(Sets &sets, std::string &error)
{
    record_repairs(sets, error);
    record_planarity(sets);
    record_annotations(sets);
    record_medial_axis(sets);
    return error.empty();
}

} // namespace probe::vr
