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
// walls.sop1's perimeter jobs (parity/sop1/walls.md), recorded by orca-o3-probe-walls
// (2026-11-13): one PerimeterGenerator set up as LayerRegion::make_perimeters sets it up, on a layer
// the probe builds, then
//
//   WLCL  process_classic (walls-2's last piece: it reaches process_no_bridge,
//         generate_lower_polygons_series and BridgeDetector, and walls-1's traverse_loops,
//         detect_steep_overhang, split_top_surfaces, apply_extra_perimeters and
//         reorient_perimeters, all of them static or private)
//   WLAR  process_arachne (walls-3's: it reaches Arachne's WallToolPaths, the walls' order, the
//         inner-outer-inner reordering, and walls-1's traverse_extrusions and clip_extrusion
//         with ClipperLib_Z, whose overhang widths come out in the paths), on every job but the
//         spiral vase ones, which make_perimeters gives only to process_classic
//
// The layers: shapes made here (squares, holes, islands, strips, triangles, overhangs, bridges,
// counterbores, top surfaces), each with the settings that reach a branch, and the OBJ meshes of
// OrcaSlicer's tests (<exe>/../src/tests/data) sliced with slice_mesh_ex every 0.2 mm, a layer with
// the one below and the one above. The regions the generator groups by fuzzy skin are one
// LayerRegion of a layer the probe makes (ProbeLayer, which reaches Layer's protected
// constructor), whose slices are the job's. A job whose settings reach a family outside domain A
// (fuzzy skin, a raft) is written with that family and no result: the port stops there with
// NotPorted.
#include "values.hpp"

#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <exception>
#include <filesystem>
#include <string>
#include <vector>

#include "libslic3r/ClipperUtils.hpp"
#include "libslic3r/Format/OBJ.hpp"
#include "libslic3r/Layer.hpp"
#include "libslic3r/PerimeterGenerator.hpp"
#include "libslic3r/Print.hpp"
#include "libslic3r/TriangleMesh.hpp"
#include "libslic3r/TriangleMeshSlicer.hpp"

namespace probe::walls {

using namespace Slic3r;

namespace {

// A layer of the probe's own: Layer's constructor is protected, for PrintObject; a layer made
// here has no object, which the generator never asks for.
class ProbeLayer : public Layer
{
public:
    ProbeLayer(size_t id, coordf_t height, coordf_t print_z, coordf_t slice_z) : Layer(id, nullptr, height, print_z, slice_z) {}
};

struct Job
{
    std::string label;
    Settings    settings;
    int         layer_id     = 5;
    double      layer_height = 0.2;
    bool        spiral       = false;
    double      rotation     = 0.;
    Surfaces    slices;
    bool        has_lower = false;
    ExPolygons  lower;
    bool        has_upper = false;
    ExPolygons  upper;
    Surfaces    upper_same;
    std::string edge;
    bool        committed = true;
};

// The settings every job starts from: explicit line widths, as a printer profile gives them.
const Settings BASE = {{"line_width", "0.42"},
                       {"inner_wall_line_width", "0.45"},
                       {"outer_wall_line_width", "0.42"},
                       {"internal_solid_infill_line_width", "0.42"},
                       {"sparse_infill_line_width", "0.45"}};

struct Flows
{
    Flow perimeter, external, overhang, solid;
};

// The flows LayerRegion::make_perimeters gives the generator, from the region's settings: the
// region's flow of each role (PrintRegion::flow) and the perimeters' bridging flow
// (LayerRegion::bridging_flow with the object's thick_bridges).
Flows flows_of(const Configs &c, int layer_id, double height)
{
    const float nozzle = float(c.print.nozzle_diameter.get_at(size_t(c.region.outer_wall_filament_id.value - 1)));
    auto        width  = [&](const ConfigOptionFloatOrPercent &w) {
        if (layer_id == 0 && c.print.initial_layer_line_width.value > 0)
            return ConfigOptionFloatOrPercent(c.print.initial_layer_line_width);
        return w.value == 0 ? ConfigOptionFloatOrPercent(c.object.line_width) : w;
    };
    Flows f;
    f.perimeter = Flow::new_from_config_width(frPerimeter, width(c.region.inner_wall_line_width), nozzle, float(height));
    f.external  = Flow::new_from_config_width(frExternalPerimeter, width(c.region.outer_wall_line_width), nozzle, float(height));
    f.solid     = Flow::new_from_config_width(frSolidInfill, width(c.region.internal_solid_infill_line_width), nozzle, float(height));
    const double bridge_width = c.region.bridge_line_width.get_abs_value(nozzle);
    const double ratio        = c.region.bridge_flow.value;
    if (c.object.thick_bridges) {
        float d = bridge_width > 0. ? float(bridge_width) : nozzle;
        if (ratio > 0.)
            d *= float(std::sqrt(ratio));
        f.overhang = Flow::bridging_flow(d, nozzle);
    } else {
        Flow base = f.perimeter;
        if (bridge_width > 0.)
            base = Flow(float(bridge_width), base.height(), nozzle);
        f.overhang = base.with_flow_ratio(ratio);
    }
    return f;
}

void run_job(Writer &w, const Job &job, bool arachne)
{
    Settings s = BASE;
    s.insert(s.end(), job.settings.begin(), job.settings.end());
    Configs c;
    make_configs(s, c);
    const double slice_z = double(job.layer_id) * job.layer_height + job.layer_height / 2.;
    auto         add     = [&](const Payload &record) {
        if (arachne)
            w.add("WLAR", record, job.committed);
        else
            w.add("WLCL", record, job.committed);
    };
    Payload p;
    p.str(job.label);
    put_settings(p, s);
    p.i32(job.layer_id).f64(job.layer_height).f64(slice_z).boolean(job.spiral).f64(job.rotation);
    Flows f;
    try {
        f = flows_of(c, job.layer_id, job.layer_height);
    } catch (const std::exception &e) {
        // No flow for these settings: the record ends here.
        p.u8(2).str(e.what());
        add(p);
        return;
    }
    p.u8(1);
    put_flow(p, f.perimeter);
    put_flow(p, f.external);
    put_flow(p, f.overhang);
    put_flow(p, f.solid);
    put_surfaces(p, job.slices);
    p.boolean(job.has_lower);
    put_expolygons(p, job.lower);
    p.boolean(job.has_upper);
    put_expolygons(p, job.upper);
    put_surfaces(p, job.upper_same);
    p.str(job.edge);
    if (!job.edge.empty()) {
        p.u8(0);
        add(p);
        return;
    }
    try {
        SurfaceCollection slices;
        slices.surfaces = job.slices;
        SurfaceCollection upper_same;
        upper_same.surfaces = job.upper_same;
        const PrintRegion region(c.region);
        ProbeLayer        layer(size_t(job.layer_id), job.layer_height, slice_z + job.layer_height / 2., slice_z);
        LayerRegion      *layerm = layer.add_region(&region);
        layerm->slices           = slices;
        const LayerRegionPtrs     compatible = {layerm};
        ExtrusionEntityCollection loops, gap_fill;
        SurfaceCollection         fill_surfaces;
        ExPolygons                fill_no_overlap;
        PerimeterGenerator g(&slices, &compatible, job.layer_height, slice_z, f.perimeter, &c.region, &c.object, &c.print, job.spiral, job.rotation, &loops,
                             &gap_fill, &fill_surfaces, &fill_no_overlap);
        g.upper_slices_same_region = nullptr;
        if (job.has_lower)
            g.lower_slices = &job.lower;
        if (job.has_upper) {
            g.upper_slices             = &job.upper;
            g.upper_slices_same_region = &upper_same;
        }
        g.layer_id           = job.layer_id;
        g.ext_perimeter_flow = f.external;
        g.overhang_flow      = f.overhang;
        g.solid_infill_flow  = f.solid;
        if (arachne)
            g.process_arachne();
        else
            g.process_classic();
        p.u8(1);
        put_entity(p, loops);
        put_entity(p, gap_fill);
        put_surfaces(p, fill_surfaces.surfaces);
        put_expolygons(p, fill_no_overlap);
        p.f64(g.ext_mm3_per_mm()).f64(g.mm3_per_mm()).f64(g.mm3_per_mm_overhang()).f64(g.smaller_width_ext_mm3_per_mm());
        put_flow(p, g.smaller_ext_perimeter_flow);
        put_polygons(p, g.lower_slices_polygons());
        for (const std::vector<Polygons> *series : {&g.m_lower_polygons_series, &g.m_external_lower_polygons_series, &g.m_smaller_external_lower_polygons_series}) {
            p.u64(series->size());
            for (const Polygons &ps : *series)
                put_polygons(p, ps);
        }
        p.boolean(g.has_fuzzy_skin).boolean(g.has_fuzzy_hole).u64(g.regions_by_fuzzify.size());
    } catch (const std::exception &e) {
        p.u8(2).str(e.what());
    }
    add(p);
}

Surfaces surfaces(const ExPolygons &es, SurfaceType type = stInternal)
{
    Surfaces out;
    for (const ExPolygon &e : es)
        out.emplace_back(type, e);
    return out;
}

// A job on the layer `shape`, with the layer below (when `lower` is given) and above.
Job job(std::string label, const ExPolygons &shape, Settings settings, int layer, const ExPolygons *lower, const ExPolygons *upper)
{
    Job j;
    j.label    = std::move(label);
    j.settings = std::move(settings);
    j.layer_id = layer;
    j.slices   = surfaces(shape);
    if (lower != nullptr) {
        j.has_lower = true;
        j.lower     = *lower;
    }
    if (upper != nullptr) {
        j.has_upper  = true;
        j.upper      = *upper;
        j.upper_same = surfaces(*upper);
    }
    return j;
}

std::vector<Job> hand_jobs()
{
    const ExPolygons sq       = {ExPolygon(rect(0, 0, 20, 20))};
    const ExPolygons sqhole   = {with_holes(rect(0, 0, 20, 20), {circle(10, 10, 5, 48)})};
    const ExPolygons islands  = {ExPolygon(rect(0, 0, 8, 8)), ExPolygon(circle(16, 4, 3, 40))};
    const ExPolygons strips   = {ExPolygon(rect(0, 0, 0.35, 15)), ExPolygon(rect(3, 0, 3.7, 12)), ExPolygon(rect(6, 0, 7.3, 12))};
    const ExPolygons tri      = {ExPolygon(poly({{0, 0}, {20, 0}, {10, 6}}))};
    const ExPolygons ell      = {ExPolygon(poly({{0, 0}, {15, 0}, {15, 1.2}, {1.2, 1.2}, {1.2, 15}, {0, 15}}))};
    const ExPolygons half     = {ExPolygon(rect(0, 0, 20, 12))};
    const ExPolygons strip8   = {ExPolygon(rect(0, 0, 20, 8))};
    const ExPolygons pads     = {ExPolygon(rect(-2, -1, 3, 9)), ExPolygon(rect(17, -1, 22, 9))};
    const ExPolygons bore     = {with_holes(rect(0, 0, 20, 20), {circle(10, 10, 2, 32)})};
    const ExPolygons bore_low = {with_holes(rect(0, 0, 20, 20), {circle(10, 10, 5, 48)})};
    const ExPolygons inner    = {ExPolygon(rect(5, 5, 15, 15))};
    const ExPolygons none     = {};
    const ExPolygons circ     = {ExPolygon(circle(10, 10, 9, 96))};
    // Three strips, each an island of its own, whose gap fill differs: 1.2 mm wide (the gap is
    // found where the inner wall vanishes, so it stays without sparse infill), 1.8 mm wide (the gap
    // is found only by the run after the last wall, which no sparse infill skips) and 1.2 mm but
    // short (its gap fill is under the 6 mm filter, the two others' over it).
    const ExPolygons gapstrips = {ExPolygon(rect(0, 0, 1.2, 15)), ExPolygon(rect(3, 0, 4.8, 12)), ExPolygon(rect(7, 0, 8.2, 4))};
    // An 8 mm and a 6 mm square joined by a 0.3 mm neck, one island: the external walls' offset
    // splits it in two (spiral vase keeps the larger).
    const ExPolygons necked = {ExPolygon(poly({{0, 0}, {8, 0}, {8, 3.85}, {9, 3.85}, {9, 1}, {15, 1}, {15, 7}, {9, 7}, {9, 4.15}, {8, 4.15}, {8, 8}, {0, 8}}))};
    // Two 0.6 mm strips, 4 and 3 mm long, joined by a 0.2 mm neck, one island narrow enough for the
    // smaller external width: its offset splits it in two narrow loops (spiral vase keeps the
    // larger).
    const ExPolygons narrow_necked = {
        ExPolygon(poly({{0, 0}, {4, 0}, {4, 0.2}, {5, 0.2}, {5, 0}, {8, 0}, {8, 0.6}, {5, 0.6}, {5, 0.4}, {4, 0.4}, {4, 0.6}, {0, 0.6}}))};
    // The upper layer covers all but a 1 mm strip along one side: the strip is narrower than the
    // default min_width_top_surface (300% of 0.45 mm) and wider than 50%.
    const ExPolygons strip_top = {ExPolygon(rect(0, 0, 20, 19))};
    std::vector<Job> out;
    out.push_back(job("square, a middle layer", sq, {}, 5, &sq, &sq));
    out.push_back(job("square, the first layer", sq, {}, 0, nullptr, &sq));
    out.push_back(job("square, the first layer, one wall", sq, {{"only_one_wall_first_layer", "1"}}, 0, nullptr, &sq));
    out.push_back(job("square, the first layer, an outer brim", sq, {{"brim_type", "outer_only"}, {"brim_width", "5"}}, 0, nullptr, &sq));
    out.push_back(job("square, the first layer, a wider first-layer line", sq, {{"initial_layer_line_width", "0.5"}}, 0, nullptr, &sq));
    out.push_back(job("square with a round hole", sqhole, {}, 3, &sqhole, &sqhole));
    out.push_back(job("two islands", islands, {}, 3, &islands, &islands));
    out.push_back(job("strips, thin walls detected", strips, {{"detect_thin_wall", "1"}}, 4, &strips, &strips));
    out.push_back(job("strips, thin walls not detected", strips, {}, 4, &strips, &strips));
    out.push_back(job("strips, three walls", strips, {{"wall_loops", "3"}}, 4, &strips, &strips));
    out.push_back(job("strips, Arachne's narrower beads", strips,
                      {{"min_bead_width", "50%"}, {"wall_transition_length", "50%"}, {"wall_distribution_count", "2"}, {"min_feature_size", "10%"}}, 4, &strips, &strips));
    out.push_back(job("triangle, three walls", tri, {{"wall_loops", "3"}}, 6, &tri, &tri));
    out.push_back(job("narrow strips, gap fill", gapstrips, {}, 6, &gapstrips, &gapstrips));
    out.push_back(job("narrow strips, no gap fill", gapstrips, {{"gap_infill_speed", "0"}}, 6, &gapstrips, &gapstrips));
    out.push_back(job("narrow strips, gap fill under 6 mm left out", gapstrips, {{"filter_out_gap_fill", "6"}}, 6, &gapstrips, &gapstrips));
    out.push_back(job("narrow strips, no sparse infill", gapstrips, {{"sparse_infill_density", "0%"}}, 6, &gapstrips, &gapstrips));
    out.push_back(job("L with narrow arms", ell, {}, 2, &ell, &ell));
    out.push_back(job("L with narrow arms, thin walls detected", ell, {{"detect_thin_wall", "1"}}, 2, &ell, &ell));
    out.push_back(job("L with narrow arms, no gap fill", ell, {{"gap_infill_speed", "0"}}, 2, &ell, &ell));
    out.push_back(job("L with narrow arms, all its gap fill under 30 mm left out", ell, {{"filter_out_gap_fill", "30"}}, 2, &ell, &ell));
    out.push_back(job("overhang on one side", sq, {}, 5, &half, &sq));
    out.push_back(job("overhang, reversed on an odd layer", sq, {{"overhang_reverse", "1"}}, 5, &half, &sq));
    out.push_back(job("overhang, reversed at threshold 0", sq, {{"overhang_reverse", "1"}, {"overhang_reverse_threshold", "0"}}, 7, &half, &sq));
    out.push_back(job("overhang, reversed, internal walls only", sq, {{"overhang_reverse", "1"}, {"overhang_reverse_internal_only", "1"}}, 7, &half, &sq));
    out.push_back(job("overhang, reversed, an even layer", sq, {{"overhang_reverse", "1"}}, 6, &half, &sq));
    out.push_back(job("overhang, reversed, a hole over the air", sqhole, {{"overhang_reverse", "1"}, {"overhang_reverse_threshold", "10%"}}, 9, &half, &sqhole));
    out.push_back(job("overhang, not detected, reversed", sq, {{"detect_overhang_wall", "0"}, {"overhang_reverse", "1"}}, 5, &half, &sq));
    out.push_back(job("overhang, extra perimeters", sq, {{"extra_perimeters_on_overhangs", "1"}}, 5, &half, &sq));
    out.push_back(job("overhang, extra perimeters, thick bridges", sq, {{"extra_perimeters_on_overhangs", "1"}, {"thick_bridges", "1"}}, 5, &half, &sq));
    out.push_back(job("bridge between two pads", strip8, {}, 4, &pads, &strip8));
    out.push_back(job("bridge between two pads, extra perimeters", strip8, {{"extra_perimeters_on_overhangs", "1"}}, 4, &pads, &strip8));
    out.push_back(job("counterbore, partial bridging", bore, {{"counterbore_hole_bridging", "partiallybridge"}}, 4, &bore_low, &bore));
    out.push_back(job("counterbore, a sacrificial layer", bore, {{"counterbore_hole_bridging", "sacrificiallayer"}}, 4, &bore_low, &bore));
    out.push_back(job("counterbore, a sacrificial layer at 30 degrees", bore, {{"counterbore_hole_bridging", "sacrificiallayer"}, {"bridge_angle", "30"}}, 4, &bore_low, &bore));
    out.push_back(job("counterbore, partial bridging at 30 degrees to the model", bore,
                      {{"counterbore_hole_bridging", "partiallybridge"}, {"bridge_angle", "30"}, {"align_infill_direction_to_model", "1"}}, 4, &bore_low, &bore));
    out.back().rotation = 0.5;
    out.push_back(job("counterbore, partial bridging, a relative angle", bore,
                      {{"counterbore_hole_bridging", "partiallybridge"}, {"bridge_angle", "30"}, {"relative_bridge_angle", "1"}}, 4, &bore_low, &bore));
    {
        // An island inside the counterbore's hole, standing on its own support.
        const ExPolygons cur  = {with_holes(rect(0, 0, 20, 20), {circle(10, 10, 4, 40)}), ExPolygon(rect(9, 9, 11, 11))};
        const ExPolygons low  = {with_holes(rect(0, 0, 20, 20), {circle(10, 10, 6, 48)}), ExPolygon(rect(8.5, 8.5, 11.5, 11.5))};
        out.push_back(job("counterbore with an island in its hole, a sacrificial layer", cur, {{"counterbore_hole_bridging", "sacrificiallayer"}}, 4, &low, &cur));
        out.push_back(job("counterbore with an island in its hole, partial bridging", cur, {{"counterbore_hole_bridging", "partiallybridge"}}, 4, &low, &cur));
    }
    out.push_back(job("counterbore over nothing", bore, {{"counterbore_hole_bridging", "sacrificiallayer"}}, 4, &none, &bore));
    out.push_back(job("top surface, one wall on top", sq, {{"only_one_wall_top", "1"}, {"wall_loops", "3"}}, 8, &sq, &inner));
    out.push_back(job("top surface, one wall on top, interface shells", sq, {{"only_one_wall_top", "1"}, {"wall_loops", "3"}, {"interface_shells", "1"}}, 8, &sq, &inner));
    out.back().upper_same = surfaces({ExPolygon(rect(3, 3, 17, 17))});
    out.push_back(job("the topmost layer, one wall on top", sq, {{"only_one_wall_top", "1"}, {"wall_loops", "3"}}, 8, &sq, nullptr));
    out.push_back(job("top surface over an overhang, one wall on top", sq, {{"only_one_wall_top", "1"}, {"wall_loops", "3"}}, 8, &half, &inner));
    out.push_back(job("top surface, a 1 mm strip, one wall on top", sq, {{"only_one_wall_top", "1"}, {"wall_loops", "3"}}, 8, &sq, &strip_top));
    out.push_back(job("top surface, a 1 mm strip, one wall on top, a narrow minimum", sq, {{"only_one_wall_top", "1"}, {"wall_loops", "3"}, {"min_width_top_surface", "50%"}}, 8, &sq,
                      &strip_top));
    out.push_back(job("top surface everywhere, one wall on top", sq, {{"only_one_wall_top", "1"}, {"wall_loops", "4"}}, 8, &sq, &none));
    out.push_back(job("outer wall first", sq, {{"wall_sequence", "outer wall/inner wall"}, {"wall_loops", "3"}}, 3, &sq, &sq));
    out.push_back(job("inner-outer-inner", sqhole, {{"wall_sequence", "inner-outer-inner wall"}, {"wall_loops", "4"}}, 3, &sqhole, &sqhole));
    out.push_back(job("inner-outer-inner, islands", islands, {{"wall_sequence", "inner-outer-inner wall"}, {"wall_loops", "4"}}, 3, &islands, &islands));
    out.push_back(job("inner-outer-inner, the first layer", sq, {{"wall_sequence", "inner-outer-inner wall"}, {"wall_loops", "4"}}, 0, nullptr, &sq));
    out.push_back(job("inner-outer-inner, no precise outer wall", sq, {{"wall_sequence", "inner-outer-inner wall"}, {"wall_loops", "4"}, {"precise_outer_wall", "0"}}, 3, &sq, &sq));
    out.push_back(job("no precise outer wall", sqhole, {{"precise_outer_wall", "0"}, {"wall_loops", "3"}}, 3, &sqhole, &sqhole));
    out.push_back(job("precise outer wall, outer wall first", sqhole, {{"wall_sequence", "outer wall/inner wall"}, {"wall_loops", "3"}}, 3, &sqhole, &sqhole));
    out.push_back(job("clockwise walls", sqhole, {{"wall_direction", "cw"}}, 3, &sqhole, &sqhole));
    out.push_back(job("arc fitting", circ, {{"enable_arc_fitting", "1"}}, 3, &circ, &circ));
    out.push_back(job("alternate extra wall, an odd layer", sq, {{"alternate_extra_wall", "1"}}, 5, &sq, &sq));
    out.push_back(job("alternate extra wall, no sparse infill", sq, {{"alternate_extra_wall", "1"}, {"sparse_infill_density", "0%"}}, 5, &sq, &sq));
    out.push_back(job("no walls", sq, {{"wall_loops", "0"}}, 3, &sq, &sq));
    out.push_back(job("one wall", sqhole, {{"wall_loops", "1"}}, 3, &sqhole, &sqhole));
    out.push_back(job("six walls", sqhole, {{"wall_loops", "6"}}, 3, &sqhole, &sqhole));
    out.push_back(job("a coarser resolution", circ, {{"resolution", "0.05"}}, 3, &circ, &circ));
    out.push_back(job("a 0.6 mm nozzle at 0.3 mm", sqhole,
                      {{"nozzle_diameter", "0.6"}, {"line_width", "0.62"}, {"inner_wall_line_width", "0.65"}, {"outer_wall_line_width", "0.62"}, {"internal_solid_infill_line_width", "0.62"}}, 3, &sqhole,
                      &sqhole));
    out.back().layer_height = 0.3;
    out.push_back(job("auto line widths", sqhole, {{"line_width", "0"}, {"inner_wall_line_width", "0"}, {"outer_wall_line_width", "0"}, {"internal_solid_infill_line_width", "0"}}, 3, &sqhole, &sqhole));
    {
        Job j = job("a surface with two extra perimeters", sq, {}, 5, &sq, &sq);
        j.slices.front().extra_perimeters = 2;
        out.push_back(j);
        Job b = job("a bottom bridge surface, one wall on top", sq, {{"only_one_wall_top", "1"}, {"wall_loops", "3"}}, 5, &half, &inner);
        b.slices.front().surface_type = stBottomBridge;
        out.push_back(b);
    }
    // Spiral vase (process_classic only: LayerRegion::make_perimeters never runs process_arachne
    // in spiral mode, so record_jobs leaves these out of WLAR). Each necked island has its twin
    // without spiral vase, which keeps both loops.
    out.push_back(job("two squares joined by a neck", necked, {}, 6, &necked, &necked));
    out.push_back(job("spiral vase, two squares joined by a neck", necked, {}, 6, &necked, &necked));
    out.back().spiral = true;
    out.push_back(job("two narrow strips joined by a neck", narrow_necked, {}, 6, &narrow_necked, &narrow_necked));
    out.push_back(job("spiral vase, two narrow strips joined by a neck", narrow_necked, {}, 6, &narrow_necked, &narrow_necked));
    out.back().spiral = true;
    // Extra perimeters over overhangs and the alternate extra wall are off in spiral vase.
    out.push_back(job("spiral vase, overhang with extra perimeters", sq, {{"extra_perimeters_on_overhangs", "1"}}, 6, &half, &sq));
    out.back().spiral = true;
    out.push_back(job("spiral vase, alternate extra wall, an odd layer", sq, {{"alternate_extra_wall", "1"}}, 5, &sq, &sq));
    out.back().spiral = true;
    // The edges: settings outside domain A, where the port stops with NotPorted.
    out.push_back(job("fuzzy skin on the outer walls", sq, {{"fuzzy_skin", "external"}}, 3, &sq, &sq));
    out.back().edge = "fuzzy-skin";
    out.push_back(job("fuzzy skin on every wall", sqhole, {{"fuzzy_skin", "all"}}, 3, &sqhole, &sqhole));
    out.back().edge = "fuzzy-skin";
    out.push_back(job("a raft of two layers", sq, {{"raft_layers", "2"}}, 2, nullptr, &sq));
    out.back().edge = "raft";
    // Left to the full set, so the committed set fits: larger layers (holes and circles of many
    // segments) whose branches a committed job reaches too, and repeats of a branch.
    const std::vector<std::string> full_only = {
        "two islands",
        "strips, thin walls not detected",
        "strips, three walls",
        "L with narrow arms, thin walls detected",
        "overhang, reversed at threshold 0",
        "bridge between two pads, extra perimeters",
        "counterbore, a sacrificial layer at 30 degrees",
        "counterbore, partial bridging at 30 degrees to the model",
        "counterbore, partial bridging, a relative angle",
        "counterbore with an island in its hole, a sacrificial layer",
        "counterbore with an island in its hole, partial bridging",
        "counterbore over nothing",
        "inner-outer-inner",
        "inner-outer-inner, islands",
        "no precise outer wall",
        "precise outer wall, outer wall first",
        "clockwise walls",
        "arc fitting",
        "one wall",
        "six walls",
        "a coarser resolution",
        "a 0.6 mm nozzle at 0.3 mm",
        "auto line widths",
        "fuzzy skin on every wall",
    };
    for (Job &j : out)
        if (std::find(full_only.begin(), full_only.end(), j.label) != full_only.end())
            j.committed = false;
    return out;
}

bool program_dir(std::filesystem::path &dir, std::string &error)
{
    char *pgm = nullptr;
    if (_get_pgmptr(&pgm) != 0 || pgm == nullptr) {
        error = "cannot find the probe's own path";
        return false;
    }
    dir = std::filesystem::path(pgm).parent_path();
    return true;
}

// The meshes of OrcaSlicer's tests: each with the layers it gives jobs for (indexes among its
// slices, 0.2 mm apart from 0.1 mm above its bottom) and how many of the first of them go to the
// committed set, with the default settings.
struct MeshPick
{
    const char      *name;
    std::vector<int> layers;
    size_t           committed;
};

bool mesh_jobs(std::vector<Job> &out, std::string &error)
{
    std::filesystem::path exe;
    if (!program_dir(exe, error))
        return false;
    const std::filesystem::path data  = exe.parent_path() / "src" / "tests" / "data";
    const std::vector<MeshPick> picks = {
        {"20mm_cube", {0, 50, 1, 98, 99}, 2},
        {"overhang", {0, 5, 20, 40, 60}, 0},
        {"bridge", {0, 10, 25, 26, 27, 40}, 0},
        {"cube_with_hole", {0, 1, 30, 98}, 0},
        {"cube_with_concave_hole", {2, 30, 60}, 0},
        {"sloping_hole", {3, 20, 45}, 0},
        {"pyramid", {0, 10, 40, 80}, 0},
        {"V", {0, 10, 20, 40}, 0},
        {"A", {0, 15, 40, 70}, 0},
        {"two_hollow_squares", {0, 5, 20}, 0},
        {"ipadstand", {0, 10, 30}, 0},
        {"small_dorito", {0, 10, 20}, 0},
        {"frog_legs", {20, 60}, 0},
        {"extruder_idler", {10, 40}, 0},
    };
    const Settings variants[] = {{}, {{"wall_loops", "3"}, {"overhang_reverse", "1"}, {"extra_perimeters_on_overhangs", "1"}, {"only_one_wall_top", "1"}}};
    for (const MeshPick &m : picks) {
        TriangleMesh mesh;
        ObjInfo      info;
        std::string  message;
        const std::filesystem::path path = data / (std::string(m.name) + ".obj");
        if (!load_obj(path.string().c_str(), &mesh, info, message)) {
            error = "cannot read " + path.string() + ": " + message;
            return false;
        }
        const BoundingBoxf3 box = mesh.bounding_box();
        std::vector<float>  zs;
        for (double z = box.min.z() + 0.1; z < box.max.z(); z += 0.2)
            zs.push_back(float(z));
        const std::vector<ExPolygons> layers = slice_mesh_ex(mesh.its, zs);
        for (size_t pick = 0; pick < m.layers.size(); ++pick) {
            const int k = m.layers[pick];
            if (k < 0 || size_t(k) >= layers.size() || layers[size_t(k)].empty())
                continue;
            const ExPolygons *lower = k > 0 ? &layers[size_t(k) - 1] : nullptr;
            const ExPolygons *upper = size_t(k) + 1 < layers.size() ? &layers[size_t(k) + 1] : nullptr;
            for (size_t v = 0; v < 2; ++v) {
                Job j     = job(std::string(m.name) + ".obj, layer " + std::to_string(k) + (v == 0 ? "" : ", three walls, overhangs reversed and extra, one wall on top"),
                                layers[size_t(k)], variants[v], k, lower, upper);
                j.committed = pick < m.committed && v == 0;
                out.push_back(std::move(j));
            }
        }
    }
    return true;
}

} // namespace

bool record_jobs(Writer &w, std::string &error)
{
    std::vector<Job> jobs = hand_jobs();
    if (!mesh_jobs(jobs, error))
        return false;
    for (const Job &j : jobs)
        run_job(w, j, false);
    // WLAR leaves out the spiral vase jobs: LayerRegion::make_perimeters runs process_classic in
    // spiral mode whatever wall_generator says.
    for (const Job &j : jobs)
        if (!j.spiral)
            run_job(w, j, true);
    return true;
}

} // namespace probe::walls
