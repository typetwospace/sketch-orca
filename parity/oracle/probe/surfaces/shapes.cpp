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
// The surfaces probe's inputs (parity/sop1/surfaces.md, added 2026-11-13 by
// orca-o3-probe-surfaces): seeded shapes, and the layers of OrcaSlicer's test meshes sliced by the
// engine's own slice_mesh_ex and typed into fill surfaces. The typing is the probe's own, simple
// rule, not detect_surfaces_type: it only has to give shapes that touch and share edges as a
// print's surfaces do. Every input is written into its record, so a replay needs none of this.
#include "shared.hpp"

#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <filesystem>

#include "libslic3r/ClipperUtils.hpp"
#include "libslic3r/Format/OBJ.hpp"
#include "libslic3r/TriangleMesh.hpp"
#include "libslic3r/TriangleMeshSlicer.hpp"

namespace probe {
namespace surfaces {

using namespace Slic3r;

Polygon rect(double x0, double y0, double x1, double y1)
{
    return Polygon(Points{Point(mm(x0), mm(y0)), Point(mm(x1), mm(y0)), Point(mm(x1), mm(y1)), Point(mm(x0), mm(y1))});
}

double uniform(SplitMix &g, double lo, double hi) { return lo + (hi - lo) * (double(g.next() >> 11) * (1.0 / 9007199254740992.0)); }

Polygon star(SplitMix &g, int n, double cx, double cy, double r)
{
    std::vector<double> a(size_t(n), 0.);
    for (double &t : a)
        t = uniform(g, 0., 6.283185307179586);
    std::sort(a.begin(), a.end());
    Polygon p;
    for (double t : a) {
        const double rr = uniform(g, r / 4, r);
        p.points.emplace_back(mm(cx + rr * std::cos(t)), mm(cy + rr * std::sin(t)));
    }
    return p;
}

Points scatter(SplitMix &g, int n, int64_t cx, int64_t cy, int64_t r)
{
    Points out;
    for (int i = 0; i < n; ++i)
        out.emplace_back(cx + int64_t(g.below(uint64_t(2 * r + 1))) - r, cy + int64_t(g.below(uint64_t(2 * r + 1))) - r);
    return out;
}

Polygon reversed(Polygon p)
{
    std::reverse(p.points.begin(), p.points.end());
    return p;
}

ExPolygons region(SplitMix &g, int n, double size)
{
    Polygons parts;
    const int k = 1 + int(g.below(uint64_t(n)));
    for (int i = 0; i < k; ++i) {
        const double x = uniform(g, 0., size * 0.8), y = uniform(g, 0., size * 0.8);
        if (g.below(3) == 0)
            parts.push_back(star(g, 3 + int(g.below(10)), x + size * 0.1, y + size * 0.1, uniform(g, size * 0.05, size * 0.3)));
        else
            parts.push_back(rect(x, y, x + uniform(g, size * 0.05, size * 0.4), y + uniform(g, size * 0.05, size * 0.4)));
    }
    return union_ex(parts);
}

namespace {

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

Polygons polys(const ExPolygons &a) { return to_polygons(a); }

void append_surfaces(Surfaces &out, const ExPolygons &exs, SurfaceType type)
{
    for (const ExPolygon &ex : exs) {
        Surface s(type, ex);
        s.thickness        = 0.2;
        s.thickness_layers = 1;
        out.push_back(s);
    }
}

} // namespace

bool mesh_layers(std::vector<Layer> &out, bool large, size_t per_mesh, std::string &error)
{
    std::filesystem::path exe;
    if (!program_dir(exe, error))
        return false;
    const std::filesystem::path data  = exe.parent_path() / "src" / "tests" / "data";
    const char *const           small_meshes[] = {"bridge", "overhang", "cube_with_hole", "cube_with_concave_hole", "two_hollow_squares", "ipadstand", "sloping_hole", "V", "A_upsidedown", "2x20x10"};
    const char *const           large_meshes[] = {"A", "extruder_idler", "frog_legs"};
    std::vector<std::string>    names(std::begin(small_meshes), std::end(small_meshes));
    if (large)
        names.insert(names.end(), std::begin(large_meshes), std::end(large_meshes));
    for (const std::string &name : names) {
        TriangleMesh      mesh;
        ObjInfo           info;
        std::string       message;
        const std::string path = (data / (name + ".obj")).string();
        if (!load_obj(path.c_str(), &mesh, info, message)) {
            error = "cannot read " + path + ": " + message;
            return false;
        }
        const BoundingBoxf3 bb = mesh.bounding_box();
        std::vector<float>  zs;
        for (double z = bb.min.z() + 0.1; z < bb.max.z(); z += 0.2)
            zs.push_back(float(z));
        const std::vector<ExPolygons> slices = slice_mesh_ex(mesh.its, zs);
        std::vector<Layer>            found;
        for (size_t n = 1; n < slices.size(); ++n) {
            const ExPolygons fill = offset_ex(slices[n], -float(mm(1.0)));
            if (fill.empty())
                continue;
            const ExPolygons bottom = opening_ex(diff_ex(fill, slices[n - 1]), float(mm(0.2)));
            const ExPolygons upper  = n + 1 < slices.size() ? slices[n + 1] : ExPolygons{};
            const ExPolygons top    = opening_ex(diff_ex(diff_ex(fill, bottom), upper), float(mm(0.2)));
            if (bottom.empty() && top.empty())
                continue;
            Polygons         outer = polys(bottom);
            polygons_append(outer, polys(top));
            const ExPolygons rest   = diff_ex(fill, outer);
            const Polygons   near   = offset(outer, float(mm(2.0)));
            const ExPolygons solid  = intersection_ex(rest, near);
            const ExPolygons sparse = diff_ex(rest, near);
            Layer            l;
            char             z[32];
            std::snprintf(z, sizeof(z), "%.2f", double(zs[n]));
            l.name = name + "@" + z;
            append_surfaces(l.surfaces, top, stTop);
            append_surfaces(l.surfaces, bottom, stBottomBridge);
            append_surfaces(l.surfaces, solid, stInternalSolid);
            append_surfaces(l.surfaces, sparse, stInternal);
            l.slices = slices[n];
            l.lower  = slices[n - 1];
            found.push_back(std::move(l));
        }
        // Spread the layers taken over the mesh's height.
        const size_t take = std::min(per_mesh, found.size());
        for (size_t i = 0; i < take; ++i)
            out.push_back(std::move(found[i * found.size() / take]));
    }
    return true;
}

} // namespace surfaces
} // namespace probe
