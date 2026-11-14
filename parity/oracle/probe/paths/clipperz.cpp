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
// paths.sop1's PSZC records (parity/sop1/paths.md): ClipperLib_Z, the Clipper of the engine's
// clipper.lib compiled with CLIPPERLIB_USE_XYZ (clipper_z.cpp), clipping paths whose points carry
// a Z, as PerimeterGenerator's clip_extrusion and LineSplit do. Each record holds the clip, the
// options and the paths, then what Execute gave: whether it threw (and the text), its result, every
// call of the Z fill callback in order with the four edge ends and the point it was given (the
// callback sets the point's Z to -(1 + the call's index)), the result paths and, for a PolyTree,
// its nodes. orca-geom replays them for the piece geom-1. Added 2026-11-13 by orca-o3-probe-paths.
#include "common.hpp"

// ClipperLib_Z before any libslic3r header (clipper_z.hpp refuses to come after clipper.hpp).
#include <clipper/clipper_z.hpp>

#include <cmath>
#include <cstdint>
#include <exception>
#include <string>
#include <vector>

namespace probe {
namespace paths {

namespace {

namespace Z = ClipperLib_Z;

void putz(Payload &p, const Z::IntPoint &q) { p.i64(q.x()).i64(q.y()).i64(q.z()); }
void putz(Payload &p, const Z::Path &path)
{
    p.u32(uint32_t(path.size()));
    for (const Z::IntPoint &q : path)
        putz(p, q);
}
void putz(Payload &p, const Z::Paths &paths)
{
    p.u32(uint32_t(paths.size()));
    for (const Z::Path &q : paths)
        putz(p, q);
}

struct Case
{
    uint8_t  clip_type    = 0; // Z::ClipType
    uint8_t  subject_open = 0;
    uint8_t  subj_fill    = 1; // Z::PolyFillType
    uint8_t  clip_fill    = 1;
    uint8_t  strict       = 0; // ioStrictlySimple
    uint8_t  zfill        = 1; // a Z fill callback is set
    uint8_t  tree         = 1; // Execute into a PolyTree (then PolyTreeToPaths), else into Paths
    Z::Paths subject, clip;
};

struct Call
{
    Z::IntPoint e1bot, e1top, e2bot, e2top, pt;
};

void zcase(Payload &p, const Case &c)
{
    p.u8(c.clip_type).u8(c.subject_open).u8(c.subj_fill).u8(c.clip_fill).u8(c.strict).u8(c.zfill).u8(c.tree);
    putz(p, c.subject);
    putz(p, c.clip);

    std::vector<Call> calls;
    Z::Clipper        clipper(c.strict ? Z::ioStrictlySimple : 0);
    if (c.zfill)
        clipper.ZFillFunction([&calls](const Z::IntPoint &e1bot, const Z::IntPoint &e1top, const Z::IntPoint &e2bot, const Z::IntPoint &e2top, Z::IntPoint &pt) {
            calls.push_back(Call{e1bot, e1top, e2bot, e2top, pt});
            pt.z() = -int64_t(calls.size());
        });
    bool        threw = false, ok = false, added_subject = false, added_clip = false;
    std::string what;
    Z::Paths    result;
    struct Node
    {
        uint32_t depth;
        bool     hole, open;
        Z::Path  contour;
    };
    std::vector<Node> nodes;
    try {
        added_subject = clipper.AddPaths(c.subject, Z::ptSubject, !c.subject_open);
        added_clip    = clipper.AddPaths(c.clip, Z::ptClip, true);
        if (c.tree) {
            Z::PolyTree tree;
            ok = clipper.Execute(Z::ClipType(c.clip_type), tree, Z::PolyFillType(c.subj_fill), Z::PolyFillType(c.clip_fill));
            for (const Z::PolyNode *n = tree.GetFirst(); n != nullptr; n = n->GetNext()) {
                uint32_t depth = 0;
                for (const Z::PolyNode *q = n->Parent; q != nullptr && q != &tree; q = q->Parent)
                    ++depth;
                nodes.push_back(Node{depth, n->IsHole(), n->IsOpen(), n->Contour});
            }
            Z::PolyTreeToPaths(std::move(tree), result);
        } else
            ok = clipper.Execute(Z::ClipType(c.clip_type), result, Z::PolyFillType(c.subj_fill), Z::PolyFillType(c.clip_fill));
    } catch (const std::exception &e) {
        threw = true;
        what  = e.what();
    }
    p.boolean(added_subject).boolean(added_clip).boolean(threw).str(what).boolean(ok);
    p.u32(uint32_t(calls.size()));
    for (const Call &k : calls) {
        putz(p, k.e1bot);
        putz(p, k.e1top);
        putz(p, k.e2bot);
        putz(p, k.e2top);
        putz(p, k.pt);
    }
    putz(p, result);
    p.u32(uint32_t(nodes.size()));
    for (const Node &n : nodes) {
        p.u32(n.depth).boolean(n.hole).boolean(n.open);
        putz(p, n.contour);
    }
}

Z::Path zrect(int64_t x0, int64_t y0, int64_t x1, int64_t y1, int64_t z)
{
    return Z::Path{Z::IntPoint(x0, y0, z), Z::IntPoint(x1, y0, z), Z::IntPoint(x1, y1, z), Z::IntPoint(x0, y1, z)};
}

// A random path of n points within r around a center, star-shaped (closed use) or a walk (open
// use), each point's Z from `zmode`: 0 all zero, 1 the path's index, 2 the point's index, 3 drawn.
Z::Path zpath(SplitMix &g, int64_t r, size_t n, bool walk, int zmode, int64_t path_index)
{
    Z::Path      out;
    const double cx = real(g, -0.5, 0.5) * double(r), cy = real(g, -0.5, 0.5) * double(r);
    double       x = cx, y = cy;
    for (size_t i = 0; i < n; ++i) {
        if (walk) {
            x += real(g, -0.6, 0.6) * double(r);
            y += real(g, -0.6, 0.6) * double(r);
        } else {
            const double a  = 2. * 3.14159265358979323846 * (double(i) + real(g, 0., 0.8)) / double(n);
            const double rr = real(g, 0.1, 1.0) * double(r);
            x               = cx + rr * std::cos(a);
            y               = cy + rr * std::sin(a);
        }
        int64_t z = 0;
        switch (zmode) {
        case 1: z = path_index; break;
        case 2: z = int64_t(i) + 1; break;
        case 3: z = rnd(g, -1000000, 1000000); break;
        default: break;
        }
        // Now and then snap to a grid, so that edges are horizontal or meet at vertices.
        const int64_t q  = (r / 8) > 0 ? r / 8 : 1;
        int64_t       xi = int64_t(std::llround(x)), yi = int64_t(std::llround(y));
        if (one_in(g, 4)) {
            xi = (xi / q) * q;
            yi = (yi / q) * q;
        }
        out.emplace_back(xi, yi, z);
    }
    return out;
}

} // namespace

void record_clipperz(Sets &sets)
{
    SplitMix g(0x5A2C11E7u);
    Emit     e(sets, "PSZC", 12, 2048);

    // Hand-made: clip_extrusion's shape (an open polyline with Z the point index through a square,
    // intersection then difference, NonZero, into a PolyTree), a closed union with and without the
    // callback, a strictly simple bow-tie, open paths into Paths (Clipper throws), squares sharing
    // an edge (horizontal edges), triangles with a Z per vertex, and coordinates past loRange.
    {
        Case c;
        c.subject_open = 1;
        c.subject      = {Z::Path{Z::IntPoint(-500, 500, 1), Z::IntPoint(500, 500, 2), Z::IntPoint(1500, 520, 3), Z::IntPoint(2500, 480, 4)}};
        c.clip         = {zrect(0, 0, 2000, 1000, 0)};
        c.clip_type    = Z::ctIntersection;
        Payload p;
        zcase(p, c);
        e.hand(p);
        c.clip_type = Z::ctDifference;
        Payload q;
        zcase(q, c);
        e.hand(q);
    }
    {
        Case c;
        c.subject   = {zrect(0, 0, 1000, 1000, 1)};
        c.clip      = {zrect(500, 500, 1500, 1500, 2)};
        c.clip_type = Z::ctUnion;
        c.tree      = 0;
        Payload p;
        zcase(p, c);
        e.hand(p);
        c.zfill = 0;
        Payload q;
        zcase(q, c);
        e.hand(q);
        c.zfill     = 1;
        c.clip_type = Z::ctIntersection;
        c.subj_fill = c.clip_fill = Z::pftEvenOdd;
        Payload r;
        zcase(r, c);
        e.hand(r);
    }
    {
        Case c;
        c.subject   = {Z::Path{Z::IntPoint(0, 0, 1), Z::IntPoint(1000, 1000, 2), Z::IntPoint(1000, 0, 3), Z::IntPoint(0, 1000, 4)}};
        c.clip_type = Z::ctUnion;
        c.strict    = 1;
        Payload p;
        zcase(p, c);
        e.hand(p);
        c.strict = 0;
        Payload q;
        zcase(q, c);
        e.hand(q);
    }
    {
        Case c;
        c.subject_open = 1;
        c.tree         = 0;
        c.subject      = {Z::Path{Z::IntPoint(-10, 5, 1), Z::IntPoint(20, 5, 2)}};
        c.clip         = {zrect(0, 0, 10, 10, 0)};
        Payload p;
        zcase(p, c);
        e.hand(p);
    }
    {
        Case c;
        c.subject   = {zrect(0, 0, 1000, 1000, 7), zrect(1000, 0, 2000, 1000, 8)};
        c.clip_type = Z::ctUnion;
        Payload p;
        zcase(p, c);
        e.hand(p);
    }
    {
        Case c;
        c.subject   = {Z::Path{Z::IntPoint(0, 0, 10), Z::IntPoint(3000, 0, 20), Z::IntPoint(1500, 2600, 30)}};
        c.clip      = {Z::Path{Z::IntPoint(0, 1700, 40), Z::IntPoint(1500, -900, 50), Z::IntPoint(3000, 1700, 60)}};
        c.clip_type = Z::ctIntersection;
        Payload p;
        zcase(p, c);
        e.hand(p);
        c.clip_type = Z::ctXor;
        Payload q;
        zcase(q, c);
        e.hand(q);
    }
    {
        Case c;
        c.subject   = {zrect(-3000000000LL, -3000000000LL, 3000000000LL, 3000000000LL, 1)};
        c.clip      = {Z::Path{Z::IntPoint(0, -4000000000LL, 2), Z::IntPoint(4000000000LL, 0, 3), Z::IntPoint(0, 4000000000LL, 4), Z::IntPoint(-4000000000LL, 0, 5)}};
        c.clip_type = Z::ctIntersection;
        Payload p;
        zcase(p, c);
        e.hand(p);
    }

    for (int i = 0; i < 800; ++i) {
        Case c;
        const int64_t r = std::vector<int64_t>{1000, 100000, 1000000, 30000000, 2000000000}[g.below(5)];
        c.clip_type     = uint8_t(g.below(4));
        c.subject_open  = one_in(g, 2) ? 1 : 0;
        c.subj_fill     = uint8_t(g.below(4));
        c.clip_fill     = uint8_t(g.below(4));
        c.strict        = one_in(g, 4) ? 1 : 0;
        c.zfill         = one_in(g, 6) ? 0 : 1;
        c.tree          = (c.subject_open || !one_in(g, 3)) ? 1 : 0;
        if (c.subject_open && one_in(g, 20))
            c.tree = 0; // Clipper throws: open paths need a PolyTree
        const int zmode = int(g.below(4));
        const int ns    = int(rnd(g, 1, 3));
        const int nc    = int(rnd(g, 0, 3));
        for (int k = 0; k < ns; ++k)
            c.subject.push_back(zpath(g, r, size_t(rnd(g, c.subject_open ? 2 : 3, 12)), c.subject_open == 1, zmode, k + 1));
        for (int k = 0; k < nc; ++k)
            c.clip.push_back(zpath(g, r, size_t(rnd(g, 3, 12)), false, zmode == 2 ? 0 : zmode, 100 + k));
        Payload p;
        zcase(p, c);
        e.random(p);
    }
}

} // namespace paths
} // namespace probe
