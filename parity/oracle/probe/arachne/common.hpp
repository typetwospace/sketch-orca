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
// What the arachne probe's sources share (parity/sop1/arachne.md, "arachne.sop1's values"): the
// writers of its compound values (points, polygons, junctions, extrusion lines, toolpaths,
// beadings, the half-edge graph), the log capture, the shapes the inputs are made of, the beading
// strategy specifications and the committed set's budget. Recorded by orca-o3-probe-arachne,
// 2026-11-13.
#pragma once

#include "../probe.hpp"

#include <cstdint>
#include <map>
#include <memory>
#include <string>
#include <utility>
#include <vector>

#include "libslic3r/Arachne/BeadingStrategy/BeadingStrategy.hpp"
#include "libslic3r/Arachne/SkeletalTrapezoidationGraph.hpp"
#include "libslic3r/Arachne/WallToolPaths.hpp"
#include "libslic3r/Arachne/utils/ExtrusionLine.hpp"
#include "libslic3r/Point.hpp"
#include "libslic3r/Polygon.hpp"

namespace probe::arachne {

using Slic3r::scaled;
using Slic3r::unscaled;
using Slic3r::Point;
using Slic3r::Points;
using Slic3r::Polygon;
using Slic3r::Polygons;
using Slic3r::Arachne::BeadingStrategy;
using Beading = Slic3r::Arachne::BeadingStrategy::Beading;
using Slic3r::Arachne::BeadingStrategyPtr;
using Slic3r::Arachne::ExtrusionJunction;
using Slic3r::Arachne::ExtrusionLine;
using Slic3r::Arachne::VariableWidthLines;
using Toolpaths = std::vector<VariableWidthLines>;
using Graph     = Slic3r::Arachne::SkeletalTrapezoidationGraph;
using Edge      = Slic3r::Arachne::STHalfEdge;
using Node      = Slic3r::Arachne::STHalfEdgeNode;

// ---------------------------------------------------------------- the committed set's budget

// One case: the records it writes, kept together so that the committed set takes a case whole or
// not at all (a replay of the committed set never meets half a case).
struct Case
{
    std::vector<std::pair<std::string, std::string>> records; // kind, payload
    size_t                                            bytes = 0;
    void add(const char (&kind)[5], const Payload &p)
    {
        records.emplace_back(std::string(kind, 4), p.bytes);
        bytes += 12 + p.bytes.size();
    }
};

// Where the cases go: every case into the full set, and a case marked committable into the
// committed set too while its family's committed budget (bytes) holds it.
class Out
{
public:
    explicit Out(Sets &sets);
    void put(const std::string &family, const Case &c, bool committable);
    // The committed bytes each family took, for the probe's log on stderr.
    std::string report() const;

private:
    Sets                         &sets_;
    std::map<std::string, size_t> left_;
    std::map<std::string, size_t> used_;
};

// ---------------------------------------------------------------- values

void put(Payload &p, const Point &q);
void put(Payload &p, const Points &pts);
void put(Payload &p, const Polygons &polys);
void put(Payload &p, const ExtrusionJunction &j);
void put(Payload &p, const ExtrusionLine &l);
void put(Payload &p, const VariableWidthLines &ls);
void put(Payload &p, const Toolpaths &t);
void put(Payload &p, const Beading &b);
void put_i64s(Payload &p, const std::vector<coord_t> &v);

// The half-edge graph, nodes and edges in their lists' order, each pointer as the index of what
// it points to in its list (-1 for none, -2 for an object no longer in the list).
void put_graph(Payload &p, Graph &g);
// A copy of `g` with every pointer moved to the copy's own nodes and edges (lists in the same order).
void copy_graph(const Graph &g, Graph &out);
// The index of `e` in `g.edges` (or of `n` in `g.nodes`), -1 for none, -2 when it is not there.
int32_t edge_index(const Graph &g, const Edge *e);
int32_t node_index(const Graph &g, const Node *n);
Edge   *edge_at(Graph &g, int32_t i);

// ---------------------------------------------------------------- logs

// Every Boost.Log record the engine writes while the probe runs, at every severity, and the text it
// writes to std::cerr; take() hands over what came since the last take().
void log_start();
void log_stop();
struct LogText
{
    std::vector<std::pair<int, std::string>> records; // severity (trace 0 ... fatal 5), message
    std::string                              cerr;
};
LogText log_take();
void    put(Payload &p, const LogText &l);

// ---------------------------------------------------------------- beading strategies

// A beading strategy as the probe builds it: either BeadingStrategyFactory::makeStrategy with its
// thirteen arguments, or a chain of strategies built one on another, the first a
// DistributedBeadingStrategy.
struct FactoryArgs
{
    coord_t outer = 0, inner = 0, transition_length = 0;
    float   transitioning_angle = 0.f;
    bool    print_thin_walls    = false;
    coord_t min_bead_width = 0, min_feature_size = 0;
    double  split = 0.5, add = 0.5;
    coord_t max_bead_count = 0, outer_wall_offset = 0;
    int     inward_distributed_center_wall_count = 2;
    double  minimum_variable_line_ratio          = 0.5;
};
struct Layer
{
    // 0 Distributed (a, b, f, g, h, radius), 1 Redistribute (a, f), 2 Widening (a, b), 3 OuterWallInset (a),
    // 4 Limited (a).
    uint8_t kind = 0;
    coord_t a = 0, b = 0;
    double  f = 0., g = 0., h = 0.;
    int     radius = 0;
};
struct StrategySpec
{
    bool               factory = true;
    FactoryArgs        args;
    std::vector<Layer> chain;
};
BeadingStrategyPtr make_strategy(const StrategySpec &s);
void               put(Payload &p, const StrategySpec &s);
// The factory's arguments as WallToolPaths::generate gives them for an outline of these widths
// (its own computation, which a WallToolPaths record checks whole): ext and inner spacing, the
// layer height, the wall count and the outer wall's inset, with the default Arachne settings of a
// nozzle of `nozzle` mm unless changed.
FactoryArgs wall_args(double ext_spacing, double inner_spacing, double nozzle, size_t inset_count, coord_t wall_0_inset);

// ---------------------------------------------------------------- shapes

// The shapes the inputs are made of, in scaled coordinates: each with a name.
struct Shape
{
    std::string name;
    Polygons    polygons;
};
Polygon            rect(double x0, double y0, double x1, double y1);
Polygon            circle(double cx, double cy, double r, int n, bool hole = false);
Polygon            star(SplitMix &g, double cx, double cy, double r_min, double r_max, int n);
std::vector<Shape> shapes();          // the probe's own shapes, small to large
std::vector<Shape> mesh_slices(std::string &error); // slices of OrcaSlicer's test meshes (src/tests/data)

// ---------------------------------------------------------------- WallToolPaths' outline preparation

// One step of the preparation WallToolPaths::generate runs on its outline (WallToolPaths.cpp
// 495-509), as an ANWH record names it: 0 simplify, 1 fixSelfIntersections, 2
// removeDegenerateVerts, 3 removeColinearEdges, 4 removeSmallAreas.
struct PrepStep
{
    uint8_t  op;
    int64_t  a = 0, b = 0; // simplify: smallest segment, allowed distance; fixSelfIntersections: epsilon
    double   f = 0.;       // removeColinearEdges: the angle; removeSmallAreas: the minimum area
    bool     holes = false; // removeSmallAreas: remove_holes
    Polygons in, out;
};
// Runs one step's function of WallToolPaths.cpp on `step.in` into `step.out`.
void run_step(PrepStep &step);
// The outline WallToolPaths::generate hands to SkeletalTrapezoidation, made by the same calls in the
// same order, with each step's input and output in `steps` when given.
Polygons prepare_outline(const Polygons &outline, coord_t smallest_segment, coord_t allowed_distance, coord_t bead_width_0, std::vector<PrepStep> *steps);

// ---------------------------------------------------------------- wall settings

// One setting of WallToolPaths as PerimeterGenerator makes it: the spacings, the wall count
// (loop_number + 1), the outer wall's inset, the layer and the Arachne options of the object's
// config (percentages of the smallest nozzle; a negative value keeps the option's default).
struct WallSetup
{
    std::string name;
    double      nozzle = 0.4, layer_height = 0.2;
    coord_t     bead_width_0 = 0, bead_width_x = 0;
    size_t      inset_count  = 3;
    coord_t     wall_0_inset = 0;
    int         layer_id     = 1;
    bool        top_or_bottom = false;
    double      min_bead_width = -1, min_feature_size = -1, transition_length = -1, filter_deviation = -1, transition_angle = -1;
    int         distribution_count = -1;
};
// The settings the WallToolPaths and SkeletalTrapezoidation records use; the first is the default.
std::vector<WallSetup> wall_setups();
// make_paths_params(layer_id, the setup's object config, a print config of its nozzle), then
// is_top_or_bottom_layer as PerimeterGenerator sets it.
Slic3r::Arachne::WallToolPathsParams wall_params(const WallSetup &s);
void                                 put(Payload &p, const WallSetup &s);
void                                 put(Payload &p, const Slic3r::Arachne::WallToolPathsParams &w);

// What WallToolPaths::generate hands to the outline preparation, the factory and
// SkeletalTrapezoidation for a setup (its own expressions, WallToolPaths.cpp 487-551).
struct StArgs
{
    FactoryArgs strat;
    coord_t     smallest_segment = 0, allowed_distance = 0;
    coord_t     discretization = 0, filter_dist = 0, filter_deviation = 0, propagation_dist = 0;
};
StArgs st_args(const WallSetup &s, const Slic3r::Arachne::WallToolPathsParams &w);

// The probe's pieces (arachne.cpp calls them in this order).
void record_beading(Out &out);
void record_lines_all(Out &out);
void record_grids(Out &out);
bool record_skeletal(Out &out, std::string &error);
bool record_walls(Out &out, std::string &error);

} // namespace probe::arachne

// The file-local helpers of WallToolPaths.cpp (not static, so the probe declares them and calls
// the engine's own definitions; never copies of them).
namespace Slic3r::Arachne {
void simplify(Slic3r::Polygons &thiss, const int64_t smallest_line_segment, const int64_t allowed_error_distance);
void fixSelfIntersections(const coord_t epsilon, Slic3r::Polygons &thiss);
void removeDegenerateVerts(Slic3r::Polygons &thiss);
void removeSmallAreas(Slic3r::Polygons &thiss, const double min_area_size, const bool remove_holes);
void removeColinearEdges(Slic3r::Polygons &thiss, const double max_deviation_angle);
} // namespace Slic3r::Arachne
