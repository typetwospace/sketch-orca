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
// The values walls.sop1's records are written in (parity/sop1/walls.md, "walls.sop1's values"):
// the compound values of parity/SOP1.md ("Values": point, polygon, expolygon, surface, polyline
// and entity) and the probe's own (a flow, settings, a line, a 3D path, an Arachne line), the
// shapes the records are made of, and the writer that keeps the committed set within its budget.
// Shared by the sources of parity/oracle/probe/walls/ (orca-o3-probe-walls, 2026-11-13).
#pragma once

#include "../probe.hpp"

// ClipperLib_Z before any libslic3r header (clipper_z.hpp refuses to come after clipper.hpp).
#include <clipper/clipper_z.hpp>

#include <cstdint>
#include <string>
#include <utility>
#include <vector>

#include "libslic3r/Arachne/utils/ExtrusionLine.hpp"
#include "libslic3r/ExPolygon.hpp"
#include "libslic3r/ExtrusionEntity.hpp"
#include "libslic3r/ExtrusionEntityCollection.hpp"
#include "libslic3r/Flow.hpp"
#include "libslic3r/Line.hpp"
#include "libslic3r/Point.hpp"
#include "libslic3r/Polygon.hpp"
#include "libslic3r/Polyline.hpp"
#include "libslic3r/PrintConfig.hpp"
#include "libslic3r/Surface.hpp"

namespace probe::walls {

using Settings = std::vector<std::pair<std::string, std::string>>;

// The writer of walls.sop1: every record goes to the full set, and a record marked committed goes to
// the committed set too. The marked records must fit the committed set's share of the file's budget
// (BUDGET): one that would not is counted in `over`, and the probe then fails (walls.cpp), so the
// committed set is always exactly the marked records.
struct Writer
{
    Sets  &sets;
    size_t committed_bytes = 0;
    size_t over            = 0;
    // The committed set's share of walls.sop1's 512 KB, with room for HEAD, PRUN and END.
    static constexpr size_t BUDGET = 500 * 1024;
    explicit Writer(Sets &s) : sets(s) {}
    void add(const char (&kind)[5], const Payload &p, bool committed)
    {
        if (committed) {
            if (committed_bytes + p.bytes.size() + 12 <= BUDGET) {
                sets.committed.add(kind, p);
                committed_bytes += p.bytes.size() + 12;
            } else {
                ++over;
            }
        }
        sets.full.add(kind, p);
    }
};

// ---- values (parity/SOP1.md, "Values") ----
void put_point(Payload &p, const Slic3r::Point &pt);
void put_points(Payload &p, const Slic3r::Points &pts);
void put_polygon(Payload &p, const Slic3r::Polygon &polygon);
void put_polygons(Payload &p, const Slic3r::Polygons &polygons);
void put_expolygon(Payload &p, const Slic3r::ExPolygon &expolygon);
void put_expolygons(Payload &p, const Slic3r::ExPolygons &expolygons);
void put_surface(Payload &p, const Slic3r::Surface &surface);
void put_surfaces(Payload &p, const Slic3r::Surfaces &surfaces);
void put_polyline(Payload &p, const Slic3r::Polyline &polyline);
void put_polylines(Payload &p, const Slic3r::Polylines &polylines);
void put_entity(Payload &p, const Slic3r::ExtrusionEntity &entity);
void put_paths(Payload &p, const Slic3r::ExtrusionPaths &paths);

// ---- the probe's own values (parity/sop1/walls.md, "walls.sop1's values") ----
// A flow: f32 width, height, spacing and nozzle diameter, bool bridge.
void put_flow(Payload &p, const Slic3r::Flow &flow);
// Settings: u64 count, each str key and str value.
void put_settings(Payload &p, const Settings &settings);
// A line: point a, point b. Lines: u64 count of them.
void put_lines(Payload &p, const Slic3r::Lines &lines);
// A 3D path (ClipperLib_Z::Path): u64 count, each i64 x, y and z. Paths: u64 count of them.
void put_zpath(Payload &p, const ClipperLib_Z::Path &path);
void put_zpaths(Payload &p, const ClipperLib_Z::Paths &paths);
// An Arachne line: u64 inset index, bool odd, bool closed, u64 count of junctions, each i64 x, y,
// i64 width and u64 perimeter index.
void put_arachne_line(Payload &p, const Slic3r::Arachne::ExtrusionLine &line);

// The three static configurations a record's settings make: each at its defaults, then applied
// from a DynamicPrintConfig with each setting set from its text (set_deserialize_strict), options
// it does not have ignored.
struct Configs
{
    Slic3r::PrintRegionConfig region;
    Slic3r::PrintObjectConfig object;
    Slic3r::PrintConfig       print;
};
void make_configs(const Settings &settings, Configs &out);

// ---- shapes, in scaled coordinates ----
Slic3r::Polygon   rect(double x0, double y0, double x1, double y1);
Slic3r::Polygon   circle(double cx, double cy, double r, int n, double phase = 0.);
Slic3r::Polygon   poly(std::initializer_list<std::pair<double, double>> mm);
Slic3r::ExPolygon with_holes(Slic3r::Polygon contour, std::vector<Slic3r::Polygon> holes);
Slic3r::Polygon   rotated(Slic3r::Polygon p, double angle, double cx = 0., double cy = 0.);
Slic3r::ExPolygon rotated(const Slic3r::ExPolygon &e, double angle, double cx = 0., double cy = 0.);
double            real(SplitMix &g, double a, double b);

// The recorders, each of its own source under walls/.
void record_moments(Writer &w);           // WLMT (basics.cpp)
void record_principal(Writer &w);         // WLPC (basics.cpp)
void record_bridging_direction(Writer &w); // WLDB (basics.cpp)
void record_bridge_detector(Writer &w);   // WLBD (basics.cpp)
void record_paths_append(Writer &w);      // WLEA (basics.cpp)
void record_reconnect(Writer &w);         // WLRC (extra.cpp)
void record_touch(Writer &w);             // WLPT (extra.cpp)
void record_sort_extra(Writer &w);        // WLSX (extra.cpp)
void record_extra_perimeters(Writer &w);  // WLXP (extra.cpp)
void record_touching(Writer &w);          // WLTP (order.cpp)
void record_compatible(Writer &w);        // WLCP (order.cpp)
bool record_jobs(Writer &w, std::string &error); // WLCL, WLAR (jobs.cpp)

} // namespace probe::walls
