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
// walls.sop1's records of what walls-3 carries besides process_arachne (parity/sop1/walls.md),
// recorded by orca-o3-probe-walls (2026-11-13):
//
//   WLTP  the inner-outer-inner reordering of Arachne's walls: findAllTouchingPerimeters,
//         reorderPerimetersByProximity and bringContoursToFront, on the walls WallToolPaths
//         makes for shapes the probe builds. PerimeterGenerator.cpp defines the three with
//         external linkage and no header declares them, nor their element type, so the probe
//         declares them below as that file has them.
//   WLCP  Layer::is_perimeter_compatible of two regions.
//
// Carries OrcaSlicer code: the declaration of PerimeterGeneratorArachneExtrusion and of the three
// functions below repeat src/libslic3r/PerimeterGenerator.cpp (OrcaSlicer 2.4.2), so their
// copyright is the holders' of that file, the OrcaSlicer contributors with Bambu Lab, Prusa Research
// and the Slic3r authors, under AGPL-3.0; taken 2026-11-13.
#include "values.hpp"

#include <algorithm>
#include <cmath>
#include <exception>
#include <map>
#include <string>
#include <unordered_set>
#include <vector>

#include "libslic3r/Arachne/WallToolPaths.hpp"
#include "libslic3r/Layer.hpp"
#include "libslic3r/Print.hpp"

namespace Slic3r {
struct PerimeterGeneratorArachneExtrusion
{
    Arachne::ExtrusionLine *extrusion = nullptr;
    bool is_contour = false;
};
std::vector<int> findAllTouchingPerimeters(const std::vector<PerimeterGeneratorArachneExtrusion> &entities, const std::unordered_set<int> &referenceIndices,
                                           size_t threshold_external, size_t threshold_internal, size_t considered_inset_idx);
std::vector<PerimeterGeneratorArachneExtrusion> reorderPerimetersByProximity(std::vector<PerimeterGeneratorArachneExtrusion> entities, size_t threshold_external,
                                                                             size_t threshold_internal);
void bringContoursToFront(std::vector<PerimeterGeneratorArachneExtrusion> &ordered_extrusions);
} // namespace Slic3r

namespace probe::walls {

using namespace Slic3r;

namespace {

coord_t sc(double mm) { return coord_t(std::llround(mm * 1000000.)); }

// The walls WallToolPaths makes for a shape, as process_arachne calls it (bead widths of the
// default flows), in the order of their perimeters and of the lines in each.
std::vector<Arachne::ExtrusionLine> walls_of(const ExPolygons &shape, int inset_count, int layer_id)
{
    Configs cfg;
    make_configs({}, cfg);
    Arachne::WallToolPathsParams params = Arachne::make_paths_params(layer_id, cfg.object, cfg.print);
    const Flow ext(0.42f, 0.2f, 0.4f), per(0.45f, 0.2f, 0.4f);
    // WallToolPaths keeps a reference to its outline: the outline outlives it.
    const Polygons         outline = to_polygons(shape);
    Arachne::WallToolPaths tp(outline, ext.scaled_spacing(), per.scaled_spacing(), size_t(inset_count), 0, 0.2, params);
    std::vector<Arachne::ExtrusionLine> out;
    for (const Arachne::VariableWidthLines &lines : tp.getToolPaths())
        for (const Arachne::ExtrusionLine &l : lines)
            out.push_back(l);
    return out;
}

void put_entries(Payload &p, const std::vector<Arachne::ExtrusionLine> &lines, const std::vector<bool> &contour)
{
    p.u64(lines.size());
    for (size_t i = 0; i < lines.size(); ++i) {
        put_arachne_line(p, lines[i]);
        p.boolean(contour[i]);
    }
}

// The index in `lines` of each entry's line.
void put_order(Payload &p, const std::vector<PerimeterGeneratorArachneExtrusion> &out, std::vector<Arachne::ExtrusionLine> &lines)
{
    p.u64(out.size());
    for (const PerimeterGeneratorArachneExtrusion &e : out) {
        p.u64(uint64_t(e.extrusion - lines.data()));
        p.boolean(e.is_contour);
    }
}

void touching_cases(Writer &w, std::vector<Arachne::ExtrusionLine> lines, bool committed)
{
    std::vector<bool> contour;
    for (const Arachne::ExtrusionLine &l : lines)
        contour.push_back(l.is_contour());
    std::vector<PerimeterGeneratorArachneExtrusion> entries;
    for (size_t i = 0; i < lines.size(); ++i)
        entries.push_back({&lines[i], bool(contour[i])});
    const Flow   ext(0.42f, 0.2f, 0.4f), per(0.45f, 0.2f, 0.4f);
    const size_t te = size_t(coord_t(ext.scaled_spacing() / 2.0 + per.scaled_spacing() / 2.0));
    const size_t te_precise = size_t(coord_t(ext.scaled_spacing() + per.scaled_spacing() / 2.0));
    const size_t ti = size_t(per.scaled_spacing());
    // findAllTouchingPerimeters from each external wall, then from the first and second walls
    // together, for the next two insets.
    for (int form = 0; form < 3; ++form) {
        if (form == 0) {
            for (size_t r = 0; r < lines.size(); ++r) {
                if (lines[r].inset_idx > 1)
                    continue;
                for (size_t inset : {size_t(1), size_t(2)}) {
                    Payload p;
                    p.u8(0);
                    put_entries(p, lines, contour);
                    // The references, in the order they are inserted into the set.
                    std::vector<int> inserted = {int(r)};
                    if (r + 1 < lines.size() && lines[r + 1].inset_idx == 1)
                        inserted.push_back(int(r + 1));
                    std::unordered_set<int> refs;
                    for (int i : inserted)
                        refs.insert(i);
                    p.u64(inserted.size());
                    for (int i : inserted)
                        p.i32(i);
                    p.u64(uint64_t(te)).u64(uint64_t(ti)).u64(uint64_t(inset));
                    std::vector<int> found = findAllTouchingPerimeters(entries, refs, te, ti, inset);
                    p.u64(found.size());
                    for (int i : found)
                        p.i32(i);
                    w.add("WLTP", p, committed && r < 2);
                }
            }
        } else if (form == 1) {
            for (size_t t : {te, te_precise}) {
                Payload p;
                p.u8(1);
                put_entries(p, lines, contour);
                p.u64(uint64_t(t)).u64(uint64_t(ti));
                put_order(p, reorderPerimetersByProximity(entries, t, ti), lines);
                w.add("WLTP", p, committed);
            }
        } else {
            Payload p;
            p.u8(2);
            put_entries(p, lines, contour);
            std::vector<PerimeterGeneratorArachneExtrusion> moved = entries;
            bringContoursToFront(moved);
            put_order(p, moved, lines);
            w.add("WLTP", p, committed);
        }
    }
}

void compatible_case(Writer &w, const Settings &a, const Settings &b, bool committed)
{
    Payload p;
    put_settings(p, a);
    put_settings(p, b);
    Configs ca, cb;
    make_configs(a, ca);
    make_configs(b, cb);
    const PrintRegion ra(ca.region), rb(cb.region);
    p.boolean(Layer::is_perimeter_compatible(ra, rb));
    w.add("WLCP", p, committed);
}

} // namespace

void record_touching(Writer &w)
{
    struct Shape
    {
        ExPolygons shape;
        int        insets;
        int        layer;
        bool       committed;
    };
    const std::vector<Shape> shapes = {
        {{ExPolygon(rect(0, 0, 10, 10))}, 4, 3, true},
        {{with_holes(rect(0, 0, 12, 12), {circle(6, 6, 2.5, 24)})}, 3, 3, false},
        {{ExPolygon(rect(0, 0, 8, 8)), ExPolygon(rect(10, 0, 18, 6))}, 3, 2, false},
        {{ExPolygon(poly({{0, 0}, {12, 0}, {12, 3}, {3, 3}, {3, 12}, {0, 12}}))}, 4, 5, false},
        {{ExPolygon(rect(0, 0, 1.3, 10))}, 3, 1, true},
        {{ExPolygon(poly({{0, 0}, {14, 0}, {7, 6}}))}, 5, 4, false},
        {{ExPolygon(rect(0, 0, 10, 10))}, 1, 3, false},
        {{with_holes(rect(0, 0, 16, 10), {rect(2, 2, 6, 8), rect(9, 2, 14, 8)})}, 3, 7, false},
    };
    for (const Shape &s : shapes) {
        std::vector<Arachne::ExtrusionLine> lines = walls_of(s.shape, s.insets, s.layer);
        touching_cases(w, lines, s.committed);
        // The same walls innermost first, as process_arachne lists them for inner/outer.
        std::reverse(lines.begin(), lines.end());
        touching_cases(w, lines, false);
    }
}

void record_compatible(Writer &w)
{
    const Settings base = {{"wall_loops", "3"}, {"inner_wall_line_width", "0.45"}, {"outer_wall_line_width", "0.42"}};
    compatible_case(w, base, base, true);
    compatible_case(w, {}, {}, true);
    const Settings changes = {
        {"outer_wall_filament_id", "2"},  {"inner_wall_filament_id", "2"},     {"wall_loops", "4"},
        {"wall_sequence", "outer wall/inner wall"}, {"is_infill_first", "1"},   {"inner_wall_speed", "61"},
        {"outer_wall_speed", "59"},       {"small_perimeter_speed", "40%"},     {"gap_infill_speed", "31"},
        {"filter_out_gap_fill", "0.5"},   {"detect_overhang_wall", "0"},        {"overhang_reverse", "1"},
        {"overhang_reverse_threshold", "40%"}, {"wall_direction", "cw"},        {"inner_wall_line_width", "0.5"},
        {"outer_wall_line_width", "100%"}, {"detect_thin_wall", "1"},           {"infill_wall_overlap", "10%"},
        {"top_bottom_infill_wall_overlap", "20%"}, {"only_one_wall_first_layer", "1"}, {"only_one_wall_top", "1"},
        {"min_width_top_surface", "200%"}, {"seam_slope_type", "external"},     {"seam_slope_conditional", "1"},
        {"scarf_angle_threshold", "100"}, {"scarf_overhang_threshold", "50%"},  {"scarf_joint_speed", "50%"},
        {"scarf_joint_flow_ratio", "0.9"}, {"seam_slope_start_height", "0.1"},  {"seam_slope_entire_loop", "1"},
        {"seam_slope_min_length", "15"},  {"seam_slope_steps", "5"},            {"seam_slope_inner_walls", "1"},
        // Settings the check leaves out: the regions stay compatible.
        {"sparse_infill_density", "40%"}, {"sparse_infill_pattern", "gyroid"},  {"top_shell_layers", "7"},
        {"counterbore_hole_bridging", "partiallybridge"}, {"extra_perimeters_on_overhangs", "1"}, {"precise_outer_wall", "0"},
        // The same value in another text.
        {"inner_wall_line_width", "0.450"}, {"wall_loops", "3"},
    };
    for (size_t i = 0; i < changes.size(); ++i) {
        Settings b = base;
        b.push_back(changes[i]);
        compatible_case(w, base, b, true);
    }
}

} // namespace probe::walls
