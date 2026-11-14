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
// model.sop1: the model as the engine loads and places it (parity/SOP1.md, "Probe files"),
// recorded by the o2-model task (2026-11-07); orca-model's tests/probes.rs replays it. Each record
// holds its inputs, then what the engine computed from them:
//
//   MXFM  Geometry::Transformation set from an offset, a rotation, a scale and a mirror, and made
//         from a matrix, with what each reads back; transform3d_from_string of a text.
//   MFAC  FacetsAnnotation::set_triangle_from_string of a list of triangles: the bit stream and
//         the used states it leaves, get_triangle_as_string of each, then reset() and the last
//         triangle set again (reset() leaves the used states).
//   MCFG  ModelConfig::set_deserialize of a key and a value: the options it leaves, or the
//         exception's text.
//   MCIR  circle_ransac of points (20 iterations) and smallest_enclosing_circle_welzl of them
//         scaled.
//   MBVL  BuildVolume of a bed outline (with extruder areas): its type, boxes, hull, circle,
//         decompositions and extruder volumes, then object_state (and volume_state_bbox for a
//         rectangle) of a box at several places.
//   MLOD  Model::read_from_file of a model file as the command line loads it (cli/run.cpp,
//         load_model: LoadModel | AddDefaultInstances, each config cut to "extruder",
//         ensure_on_bed): the error, or the model; then the object's boxes, hulls and z, two
//         instances added, ensure_on_bed, the print volume states against five beds (named by
//         their index among MBVL's, which hold them), each instance's arrange polygon
//         (get_instance_arrange_poly) and a result applied, and center_around_origin.
//   M3MF  Model::read_from_file of a 3MF file as the command line loads it (load_bbs_3mf; the plate
//         data, presets, BBL flag and version it asks for are released unread): the engine's
//         log of the read as stderr gets it (ReadLog: error and fatal, the importer's add_error
//         texts among them), then the error, or what the read leaves that the cuts drop or
//         MLOD's model leaves out (the model's design, profile and model information, each
//         object's module, whole configuration, layer height profile, height ranges and brim
//         ears, each instance's loaded id and assembly transformation, each part's whole
//         configuration) and the model as MLOD records it after the cuts; for some files MLOD's
//         steps after it. The temporary folder is written <dir> in the log and the error. Each
//         names the piece of the 3MF reader's port it needs last (1: the container, its
//         relationships and the root model; 2: the config entries; 3: the sub-model files and
//         the assembly; a file that loads needs piece 3).
//   MARP  get_bed_shape, update_arrange_params, get_shrink_bedpts and the items' inflation.
//   MTMP  Print::get_filament_temp_type and get_compatible_filament_type.
//
// The model files: generated boxes (binary and ASCII STL, one with the .oltp header), files the
// readers refuse, OrcaSlicer's test STL files but the CR box (from the oracle's source tree,
// <exe>/../src/tests/data; the CR box kills the engine, parity/CORPUS.md), its 21 OBJ meshes as
// binary STL, made as Sketch makes model.stl (parity/CORPUS.md), and as they are; the fixtures of
// fixtures/model/ (a folder each: model.* and the files beside it, such as an OBJ's material
// library); OBJ files made here (line ends, NULs and Ctrl-Z, lines past the reader's 64 KiB
// pieces, numbers, refused faces); and, in the full set only, the corpus's model files
// (<work>/parity/corpus/meshes) but the CR box. A file is written to a temporary folder under a
// plain ASCII name, which names the object, with its files beside it; the committed set takes the
// files of 4000 bytes or fewer (of the random boxes, the first eight; of OrcaSlicer's OBJ meshes,
// each once). The 3MF reader keeps a backup folder under temporary_dir(), which the command line
// sets to the system's temporary folder; here it is the probe's own temporary folder. The tasks
// run under the oracle's threading: global_control(max_allowed_parallelism, 1) inside a
// task_arena (parity/oracle/README.md).
//
// The 3MF files: the fixtures of fixtures/model/ named 3mf-<piece>-<what>, each a folder model.3mf/
// of the archive's entries that pack_zip packs here (marker files beside it can give an entry a CRC
// one bit off or an uncompressed size it does not have, store it rather than deflate it, or pack a
// second entry of its name); archives the reader refuses, made from the plain fixture's entries;
// variants of the 3mf-2-numbers fixture with one number in another form; and, in the full set only,
// the corpus's 3MF and OrcaSlicer's own 3MF projects (<exe>/../src/resources). The committed set
// takes the fixtures (but those with a full-only file), the refused archives (but the two cut ones)
// and the variants marked committed, each of 4000 bytes or fewer, for the committed file's budget.
// MLOD's steps run for the Bambu fixture and the full set's files, from their own generator
// (0x30DE9000), so MLOD's records stay as they were.
#include "probe.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <set>
#include <sstream>
#include <stdexcept>
#include <string>
#include <tuple>
#include <vector>

#include <process.h>

#include <boost/core/null_deleter.hpp>
#include <boost/log/core.hpp>
#include <boost/log/expressions.hpp>
#include <boost/log/sinks/sync_frontend.hpp>
#include <boost/log/sinks/text_ostream_backend.hpp>
#include <boost/log/trivial.hpp>
#include <boost/make_shared.hpp>
#include <boost/shared_ptr.hpp>

#include <oneapi/tbb/global_control.h>
#include <oneapi/tbb/task_arena.h>

#include <libslic3r/Arrange.hpp>
#include <libslic3r/BuildVolume.hpp>
#include <libslic3r/Geometry.hpp>
#include <libslic3r/Geometry/Circle.hpp>
#include <libslic3r/Model.hpp>
#include <libslic3r/ModelArrange.hpp>
#include <libslic3r/Format/bbs_3mf.hpp>
#include <libslic3r/Preset.hpp>
#include <libslic3r/Print.hpp>
#include <libslic3r/PrintConfig.hpp>
#include <libslic3r/Semver.hpp>
#include <libslic3r/Utils.hpp>
#include <libslic3r/miniz_extension.hpp>

namespace probe {

namespace {

using namespace Slic3r;

uint64_t fnv1a(const std::string &s)
{
    uint64_t h = 0xcbf29ce484222325ull;
    for (unsigned char c : s) {
        h ^= c;
        h *= 0x100000001b3ull;
    }
    return h;
}

double real(SplitMix &g, double a, double b) { return a + (b - a) * (double(g.next() >> 11) * 0x1.0p-53); }

void add(Sets &sets, const char (&kind)[5], const Payload &p, bool committed)
{
    if (committed)
        sets.committed.add(kind, p);
    sets.full.add(kind, p);
}

void put_vec3(Payload &p, const Vec3d &v)
{
    for (int i = 0; i < 3; ++i)
        p.f64(v(i));
}
void put_mat(Payload &p, const Transform3d &m)
{
    for (int r = 0; r < 4; ++r)
        for (int c = 0; c < 4; ++c)
            p.f64(m.matrix()(r, c));
}
void put_bb3(Payload &p, const BoundingBoxf3 &b)
{
    p.boolean(b.defined);
    put_vec3(p, b.min);
    put_vec3(p, b.max);
}
void put_bb(Payload &p, const BoundingBox &b) { p.boolean(b.defined).i64(b.min.x()).i64(b.min.y()).i64(b.max.x()).i64(b.max.y()); }
void put_points(Payload &p, const Points &pts)
{
    p.u64(pts.size());
    for (const Point &pt : pts)
        p.i64(pt.x()).i64(pt.y());
}
void put_vec2s(Payload &p, const std::vector<Vec2d> &pts)
{
    p.u64(pts.size());
    for (const Vec2d &v : pts)
        p.f64(v.x()).f64(v.y());
}
void put_circle(Payload &p, const Geometry::Circled &c) { p.f64(c.center.x()).f64(c.center.y()).f64(c.radius); }
uint64_t its_hash(const indexed_triangle_set &its)
{
    Payload p;
    p.u64(its.vertices.size());
    for (const stl_vertex &v : its.vertices)
        for (int k = 0; k < 3; ++k)
            p.f32(v(k));
    p.u64(its.indices.size());
    for (const stl_triangle_vertex_indices &f : its.indices)
        for (int k = 0; k < 3; ++k)
            p.i32(f(k));
    return fnv1a(p.bytes);
}
// "key=value\n" for each option, in key order.
std::string config_dump(const ModelConfig &c)
{
    std::string out;
    for (const std::string &k : c.keys())
        out += k + "=" + c.opt_serialize(k) + "\n";
    return out;
}

// ---- MXFM ----------------------------------------------------------------------------------

void record_xfm(Sets &sets)
{
    SplitMix          g(0x30DE1000u);
    const char *const texts[] = {"1 0 0 0 0 1 0 0 0 0 1 0 10 20 30 1",
                                 "0 -1 0 0 1 0 0 0 0 0 1 0 5.5 -2 0 1",
                                 " 1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1",
                                 "1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 ",
                                 "2  0 0 0 0 2 0 0 0 0 2 0 1e3 x 7junk 1",
                                 "1 0 0 0 1 0 0 0 1 10 20 30",
                                 "",
                                 "0.5\t0 0 0 0 0.5 0 0 0 0 0.5 0 1 2 3 1",
                                 "1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0",
                                 "-1 0 0 0 0 1 0 0 0 0 1 0 +4 -.5 3. 1",
                                 "0x10 0 0 0 0 1 0 0 0 0 1 0 1e-3 1E+2 inf 1"};
    for (int i = 0; i < 160; ++i) {
        Payload p;
        Vec3d   off(real(g, -200, 200), real(g, -200, 200), real(g, -50, 50));
        Vec3d   rot(g.below(3) == 0 ? 0. : real(g, -7, 7), g.below(3) == 0 ? 0. : real(g, -7, 7), real(g, -7, 7));
        Vec3d   scl(real(g, 0.05, 4), real(g, 0.05, 4), real(g, 0.05, 4));
        if (g.below(3) == 0)
            scl = Vec3d(scl.x(), scl.x(), scl.x());
        Vec3d mir(g.below(4) == 0 ? -1. : 1., g.below(4) == 0 ? -1. : 1., g.below(4) == 0 ? -1. : 1.);
        // A matrix as a 3MF file gives one: a rotation, a scale (sometimes uneven or mirrored) and
        // an offset, now and then with a shear.
        Transform3d raw = Transform3d::Identity();
        raw.translate(Vec3d(real(g, -100, 100), real(g, -100, 100), real(g, -100, 100)));
        raw.rotate(Eigen::AngleAxisd(real(g, -3.2, 3.2), Vec3d::UnitZ()) * Eigen::AngleAxisd(real(g, -3.2, 3.2), Vec3d::UnitY()) *
                   Eigen::AngleAxisd(real(g, -3.2, 3.2), Vec3d::UnitX()));
        raw.scale(Vec3d(real(g, 0.1, 3) * (g.below(5) == 0 ? -1. : 1.), real(g, 0.1, 3), real(g, 0.1, 3)));
        if (g.below(4) == 0)
            raw.matrix()(int(g.below(3)), int(g.below(3))) += real(g, -0.5, 0.5);
        if (i < 6) {
            raw = Transform3d::Identity();
            if (i == 1)
                raw.rotate(Eigen::AngleAxisd(PI / 2, Vec3d::UnitZ()));
            if (i == 2)
                raw.scale(Vec3d(-1, 1, 1));
            if (i == 3)
                raw.scale(2.);
            if (i == 4)
                raw.translate(Vec3d(10, -20, 5));
            if (i == 5)
                raw.rotate(Eigen::AngleAxisd(PI, Vec3d::UnitX()));
        }
        const std::string text = i < int(std::size(texts)) ? texts[i] : "";
        put_vec3(p, off);
        put_vec3(p, rot);
        put_vec3(p, scl);
        put_vec3(p, mir);
        put_mat(p, raw);
        p.str(text);
        Geometry::Transformation t;
        t.set_offset(off);
        t.set_rotation(rot);
        t.set_scaling_factor(scl);
        t.set_mirror(mir);
        put_mat(p, t.get_matrix());
        put_mat(p, t.get_matrix_no_offset());
        put_vec3(p, t.get_offset());
        put_vec3(p, t.get_rotation());
        put_vec3(p, t.get_scaling_factor());
        put_vec3(p, t.get_mirror());
        p.boolean(t.is_left_handed());
        Geometry::Transformation u(raw);
        put_vec3(p, u.get_offset());
        put_vec3(p, u.get_rotation());
        put_vec3(p, u.get_scaling_factor());
        put_vec3(p, u.get_mirror());
        p.boolean(u.is_left_handed());
        put_mat(p, u.get_matrix_no_offset());
        put_mat(p, Geometry::transform3d_from_string(text));
        add(sets, "MXFM", p, i < 40);
    }
}

// ---- MFAC ----------------------------------------------------------------------------------

indexed_triangle_set unit_cube()
{
    indexed_triangle_set its;
    its.vertices = {{0, 0, 0}, {1, 0, 0}, {1, 1, 0}, {0, 1, 0}, {0, 0, 1}, {1, 0, 1}, {1, 1, 1}, {0, 1, 1}};
    its.indices  = {{0, 2, 1}, {0, 3, 2}, {4, 5, 6}, {4, 6, 7}, {0, 1, 5}, {0, 5, 4}, {1, 2, 6}, {1, 6, 5}, {2, 3, 7}, {2, 7, 6}, {3, 0, 4}, {3, 4, 7}};
    return its;
}

void put_facets(Payload &p, const FacetsAnnotation &f)
{
    const TriangleSelector::TriangleSplittingData &d = f.get_data();
    p.u64(d.triangles_to_split.size());
    for (const auto &m : d.triangles_to_split)
        p.i32(m.triangle_idx).i32(m.bitstream_start_idx);
    p.u64(d.bitstream.size());
    for (bool b : d.bitstream)
        p.boolean(b);
    p.u64(d.used_states.size());
    for (bool b : d.used_states)
        p.boolean(b);
}

void record_fac(Sets &sets)
{
    SplitMix     g(0x30DE2000u);
    Model        model;
    ModelObject *object = model.add_object();
    const char   digits[] = "0123456789ABCDEF";
    for (int i = 0; i < 60; ++i) {
        Payload                                  p;
        std::vector<std::pair<int, std::string>> tris;
        int                                      id = int(g.below(4));
        const int                                n  = 1 + int(g.below(6));
        size_t                                   bits = 0;
        for (int k = 0; k < n; ++k) {
            std::string s;
            const int   len = 1 + int(g.below(9));
            for (int c = 0; c < len; ++c)
                s.push_back(g.below(40) == 0 ? "a?x "[g.below(4)] : digits[g.below(16)]);
            // A string whose first character is C asks update_used_states for a nibble past the
            // stream's end. Within the last word of the vector<bool> that nibble is 0; when the
            // stream ends on a word (32 bits) the C++ reads past its storage (parity/HAZARDS.md,
            // orca-model), so such a string is not recorded.
            bits += 4 * s.size();
            if (s[0] == 'C' && (bits % 32 == 0 || (4 * s.size()) % 32 == 0))
                s[0] = '4';
            tris.emplace_back(id, s);
            id += 1 + int(g.below(5));
        }
        p.u64(tris.size());
        for (const auto &t : tris)
            p.i32(t.first).str(t.second);
        p.i32(id); // a triangle the list does not hold
        ModelVolume *vol = object->add_volume(TriangleMesh(unit_cube()));
        for (const auto &t : tris)
            vol->mmu_segmentation_facets.set_triangle_from_string(t.first, t.second);
        put_facets(p, vol->mmu_segmentation_facets);
        for (const auto &t : tris)
            p.str(vol->mmu_segmentation_facets.get_triangle_as_string(t.first));
        p.str(vol->mmu_segmentation_facets.get_triangle_as_string(id));
        vol->mmu_segmentation_facets.reset();
        vol->mmu_segmentation_facets.set_triangle_from_string(tris.back().first, tris.back().second);
        put_facets(p, vol->mmu_segmentation_facets);
        add(sets, "MFAC", p, i < 30);
    }
}

// ---- MCFG ----------------------------------------------------------------------------------

void record_cfg(Sets &sets)
{
    const std::pair<const char *, const char *> cases[] = {
        {"extruder", "2"},
        {"extruder", "0"},
        {"extruder", "x"},
        {"extruder", "3.7"},
        {"extruder", " 4"},
        {"extruder", ""},
        {"layer_height", "0.2"},
        {"layer_height", "abc"},
        {"wall_loops", "3"},
        {"sparse_infill_density", "15%"},
        {"enable_support", "1"},
        {"enable_support", "yes"},
        {"enable_support", "true"},
        {"enable_support", "2"},
        {"support_type", "tree(auto)"},
        {"support_type", "no_such_type"},
        {"brim_type", "outer_only"},
        {"no_such_key", "1"},
        {"wall_filament", "2"},
        {"sparse_infill_pattern", "gyroid"},
        {"sparse_infill_pattern", "zigzag?"},
        {"top_shell_layers", ""},
        {"seam_position", "aligned"},
        {"outer_wall_speed", "nil"},
        {"fuzzy_skin", "external"},
        {"infill_direction", "45"},
        {"raft_layers", "-1"},
        {"compatible_printers", "a;b"},
        {"wipe_tower_x", "1;2"},
        {"wipe_tower_x", "1,2"},
        {"filament_colour", "#FF0000"},
        {"wall_sequence", "inner wall/outer wall"},
        {"perimeters", "4"},
        {"infill_every_layers", "2"},
    };
    for (const auto &c : cases) {
        Payload p;
        p.str(c.first).str(c.second);
        ModelConfig               cfg;
        ConfigSubstitutionContext ctx(ForwardCompatibilitySubstitutionRule::EnableSilent);
        try {
            cfg.set_deserialize(c.first, c.second, ctx);
            p.u8(0).str(config_dump(cfg));
        } catch (const std::exception &e) {
            p.u8(1).str(e.what());
        }
        add(sets, "MCFG", p, true);
    }
}

// ---- MCIR ----------------------------------------------------------------------------------

std::vector<Vec2d> round_bed(int n, double r, double cx, double cy, double quantum, double ry)
{
    std::vector<Vec2d> pts;
    for (int k = 0; k < n; ++k) {
        const double a = 2. * PI * double(k) / double(n);
        double       x = cx + r * std::cos(a), y = cy + ry * std::sin(a);
        if (quantum > 0) {
            x = std::round(x / quantum) * quantum;
            y = std::round(y / quantum) * quantum;
        }
        pts.emplace_back(x, y);
    }
    return pts;
}

void record_cir(Sets &sets)
{
    SplitMix g(0x30DE3000u);
    for (int i = 0; i < 80; ++i) {
        Payload            p;
        std::vector<Vec2d> pts;
        const int          kind = int(g.below(4));
        if (kind == 0) {
            const int n = 3 + int(g.below(40));
            for (int k = 0; k < n; ++k)
                pts.emplace_back(real(g, -100, 100), real(g, -100, 100));
        } else {
            const double r       = real(g, 20, 200);
            const int    n       = 4 + int(g.below(150));
            const double cx      = real(g, -50, 150);
            const double cy      = real(g, -50, 150);
            const double quantum = g.below(3) == 0 ? 0. : (g.below(2) ? 0.01 : 0.1);
            const double ry      = kind == 3 ? r * real(g, 0.8, 1.2) : r;
            pts                  = round_bed(n, r, cx, cy, quantum, ry);
        }
        if (i < 3)
            pts.resize(size_t(i));
        put_vec2s(p, pts);
        double            err = -1;
        Geometry::Circled c   = Geometry::circle_ransac(pts, 20, &err);
        put_circle(p, c);
        p.f64(err);
        const Points sp = Polygon::new_scale(pts).points;
        if (!sp.empty())
            put_circle(p, Geometry::smallest_enclosing_circle_welzl(sp));
        add(sets, "MCIR", p, i < 24);
    }
}

// ---- MBVL ----------------------------------------------------------------------------------

struct Bed
{
    std::vector<Vec2d>              area;
    double                          height = 0;
    std::vector<std::vector<Vec2d>> extruders;
    std::vector<double>             extruder_heights;
};

std::vector<Vec2d> rect(double x0, double y0, double x1, double y1) { return {{x0, y0}, {x1, y0}, {x1, y1}, {x0, y1}}; }

std::vector<Bed> beds()
{
    SplitMix         g(0x30DE4000u);
    std::vector<Bed> out;
    out.push_back({rect(0, 0, 256, 256), 250, {}, {}});
    out.push_back({rect(-128, -128, 128, 128), 100, {}, {}});
    out.push_back({round_bed(36, 90, 0, 0, 0.01, 90), 200, {}, {}});
    out.push_back({{{0, 0}, {200, 0}, {260, 100}, {200, 200}, {0, 200}, {-60, 100}}, 200, {}, {}});
    out.push_back({{{0, 0}, {200, 0}, {200, 100}, {100, 100}, {100, 200}, {0, 200}}, 0, {}, {}});
    out.push_back({rect(0, 0, 350, 320), 325, {rect(0, 0, 325, 320), rect(25, 0, 350, 320)}, {325, 320}});
    out.push_back({rect(0, 0, 256, 256), 0, {rect(0, 0, 256, 256)}, {0}});
    out.push_back({{{0, 0}, {100, 0}, {50, 80}}, 50, {}, {}});
    out.push_back({{{0, 0}, {100, 10}, {90, 110}, {-10, 100}}, 120, {}, {}});
    out.push_back({round_bed(36, 60, 100, 100, 0.1, 60), 150, {}, {}});
    out.push_back({round_bed(144, 120, 0, 0, 0, 120), 300, {}, {}});
    out.push_back({round_bed(72, 100, 0, 0, 0.01, 80), 200, {}, {}});
    out.push_back({rect(0, 0, 300, 300), 300, {round_bed(72, 150, 150, 150, 0.01, 150), rect(0, 0, 300, 300)}, {300, 250}});
    for (int i = 0; i < 20; ++i) {
        Bed       b;
        const int kind = int(g.below(4));
        if (kind == 0) {
            const double x0 = real(g, -100, 0), y0 = real(g, -100, 0), x1 = real(g, 50, 400), y1 = real(g, 50, 400);
            b.area = rect(x0, y0, x1, y1);
        } else if (kind == 1) {
            const int    n  = 8 + int(g.below(100));
            const double r  = real(g, 30, 200);
            const double cx = real(g, -20, 20), cy = real(g, -20, 20);
            const double q  = g.below(2) ? 0.01 : 0.;
            const double ry = real(g, 30, 200);
            b.area          = round_bed(n, r, cx, cy, q, g.below(2) ? r : ry);
        } else {
            const int n = 3 + int(g.below(6));
            for (int k = 0; k < n; ++k) {
                const double a = 2. * PI * (double(k) + real(g, 0, 0.6)) / double(n);
                const double r = kind == 2 ? 100. : real(g, 40, 140);
                b.area.emplace_back(r * std::cos(a), r * std::sin(a));
            }
        }
        b.height = g.below(5) == 0 ? 0. : real(g, 50, 400);
        out.push_back(b);
    }
    return out;
}

void put_bed(Payload &p, const Bed &b)
{
    put_vec2s(p, b.area);
    p.f64(b.height);
    p.u64(b.extruders.size());
    for (size_t i = 0; i < b.extruders.size(); ++i) {
        put_vec2s(p, b.extruders[i]);
        p.f64(b.extruder_heights[i]);
    }
}

void record_bvl(Sets &sets)
{
    SplitMix                   g(0x30DE5000u);
    const indexed_triangle_set cube = unit_cube();
    int                        i    = 0;
    for (const Bed &b : beds()) {
        Payload p;
        put_bed(p, b);
        BuildVolume bv(b.area, b.height, b.extruders, b.extruder_heights);
        p.i32(int(bv.type()));
        p.str(std::string(bv.type_name()));
        put_bb(p, bv.bounding_box());
        put_bb3(p, bv.bounding_volume());
        put_points(p, bv.convex_hull().points);
        put_points(p, bv.polygon().points);
        put_circle(p, bv.circle());
        put_vec2s(p, bv.top_bottom_convex_hull_decomposition_scene().first);
        put_vec2s(p, bv.top_bottom_convex_hull_decomposition_scene().second);
        put_vec2s(p, bv.top_bottom_convex_hull_decomposition_bed().first);
        put_vec2s(p, bv.top_bottom_convex_hull_decomposition_bed().second);
        p.u64(uint64_t(bv.get_extruder_area_count()));
        for (int k = 0; k < bv.get_extruder_area_count(); ++k) {
            const BuildVolume::BuildExtruderVolume &ev = bv.get_extruder_area_volume(k);
            p.boolean(ev.same_with_bed).i32(int(ev.type));
            put_bb(p, ev.bbox);
            put_bb3(p, ev.bboxf);
            put_circle(p, ev.circle);
        }
        const BoundingBoxf3 vol = bv.bounding_volume();
        for (int k = 0; k < 24; ++k) {
            const double s  = real(g, 1, 120);
            Vec3d        at = Vec3d(real(g, vol.min.x() - 80, vol.max.x() + 10), real(g, vol.min.y() - 80, vol.max.y() + 10), real(g, -1.5 * s, 30));
            if (k % 6 == 0)
                at.z() = 0;
            Transform3d t = Transform3d::Identity();
            t.translate(at);
            if (g.below(3) == 0)
                t.rotate(Eigen::AngleAxisd(real(g, -3, 3), Vec3d::UnitZ()));
            const double sy = s * real(g, 0.2, 1.5), sz = s * real(g, 0.1, 3);
            t.scale(Vec3d(s, sy, sz));
            const bool below  = g.below(4) != 0;
            const bool bottom = g.below(4) != 0;
            put_mat(p, t);
            p.boolean(below).boolean(bottom);
            p.u8(uint8_t(bv.object_state(cube, t.cast<float>(), below, bottom)));
            if (bv.type() == BuildVolume_Type::Rectangle) {
                const TriangleMesh m(cube);
                p.u8(uint8_t(bv.volume_state_bbox(m.transformed_bounding_box(t), bottom)));
            }
        }
        add(sets, "MBVL", p, i < 10);
        ++i;
    }
}

// ---- Model files ---------------------------------------------------------------------------

struct Tri
{
    float v[3][3];
};

std::string binary_stl(const std::vector<Tri> &tris, const char *header, size_t header_length = 80)
{
    std::string out(header_length, '\0');
    std::memcpy(out.data(), header, std::strlen(header));
    Payload p;
    p.u32(uint32_t(tris.size()));
    for (const Tri &t : tris) {
        double a[3], b[3], c[3];
        for (int k = 0; k < 3; ++k) {
            a[k] = t.v[0][k];
            b[k] = t.v[1][k];
            c[k] = t.v[2][k];
        }
        const double u[3] = {b[0] - a[0], b[1] - a[1], b[2] - a[2]}, w[3] = {c[0] - a[0], c[1] - a[1], c[2] - a[2]};
        const double n[3] = {u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]};
        const double l    = std::sqrt(n[0] * n[0] + n[1] * n[1] + n[2] * n[2]);
        for (int k = 0; k < 3; ++k)
            p.f32(l > 0 ? float(n[k] / l) : 0.f);
        for (int i = 0; i < 3; ++i)
            for (int k = 0; k < 3; ++k)
                p.f32(t.v[i][k]);
        p.u16(0);
    }
    return out + p.bytes;
}

std::string ascii_stl(const std::vector<Tri> &tris, const char *le)
{
    std::string out = std::string("solid probe") + le;
    char        buf[256];
    for (const Tri &t : tris) {
        out += std::string("  facet normal 0 0 0") + le + "    outer loop" + le;
        for (const auto &v : t.v) {
            std::snprintf(buf, sizeof(buf), "      vertex %.9g %.9g %.9g", double(v[0]), double(v[1]), double(v[2]));
            out += buf;
            out += le;
        }
        out += std::string("    endloop") + le + "  endfacet" + le;
    }
    return out + "endsolid probe" + le;
}

std::vector<Tri> box(const double o[3], const double s[3])
{
    static const int f[12][3] = {{0, 2, 1}, {0, 3, 2}, {4, 5, 6}, {4, 6, 7}, {0, 1, 5}, {0, 5, 4}, {1, 2, 6}, {1, 6, 5}, {2, 3, 7}, {2, 7, 6}, {3, 0, 4}, {3, 4, 7}};
    static const int c[8][3]  = {{0, 0, 0}, {1, 0, 0}, {1, 1, 0}, {0, 1, 0}, {0, 0, 1}, {1, 0, 1}, {1, 1, 1}, {0, 1, 1}};
    std::vector<Tri> out;
    for (const auto &face : f) {
        Tri t;
        for (int i = 0; i < 3; ++i)
            for (int k = 0; k < 3; ++k)
                t.v[i][k] = float(o[k] + c[face[i]][k] * s[k]);
        out.push_back(t);
    }
    return out;
}

void rotate_all(std::vector<Tri> &tris, double ax, double ay, double az)
{
    Transform3d m = Transform3d::Identity();
    m.rotate(Eigen::AngleAxisd(az, Vec3d::UnitZ()) * Eigen::AngleAxisd(ay, Vec3d::UnitY()) * Eigen::AngleAxisd(ax, Vec3d::UnitX()));
    for (Tri &t : tris)
        for (auto &v : t.v) {
            const Vec3d q = m * Vec3d(v[0], v[1], v[2]);
            for (int k = 0; k < 3; ++k)
                v[k] = float(q(k));
        }
}

// An entry of a zip archive: its name and its bytes; a fixture's marker files (fixture_3mf) can
// have it packed with a CRC one bit off, with an uncompressed size its data does not have, or
// stored rather than deflated.
struct ZipEntry
{
    std::string name, data;
    bool        wrong_crc    = false;
    uint32_t    claimed_size = 0; // 0: the data's own size
    // A stored entry's claim set from the archive's size (pack_zip): 1 the largest claim miniz's
    // reader opens the archive with, 2 one byte more.
    int  claim_end = 0;
    bool stored    = false;
};

struct ModelCase
{
    std::string label;
    std::string name;
    std::string bytes;
    // Files written beside it, by name (an OBJ's material libraries).
    std::vector<std::pair<std::string, std::string>> extra;
    // Recorded in the full set only.
    bool full_only = false;
    // A 3MF file (M3MF): the piece of the reader's port it tests (1 to 3), the entries a fixture
    // packs, and whether MLOD's steps run after the read.
    int                   piece = 0;
    std::vector<ZipEntry> entries;
    bool                  steps = false;
};

void put16(std::string &s, uint32_t v)
{
    s.push_back(char(v & 0xFF));
    s.push_back(char((v >> 8) & 0xFF));
}
void put32(std::string &s, uint32_t v)
{
    put16(s, v & 0xFFFF);
    put16(s, v >> 16);
}

void set32(std::string &s, size_t at, uint32_t v)
{
    for (int k = 0; k < 4; ++k)
        s[at + k] = char((v >> (8 * k)) & 0xFF);
}

// A zip archive of the entries in their order: each deflated (miniz's tdefl at level 6, raw), or
// stored when empty or marked stored; version 20, no flags, extra fields or comments, and a fixed
// date (2026-11-10, 00:00). miniz's own writer is not used: it stamps entries with the local time.
// `spans`, when given, gets where each entry's data starts in the archive and its length; the
// entry numbered `wrong_crc`, and each entry marked wrong_crc, gets a CRC one bit off; an entry's
// claimed_size, when set, is written as its uncompressed size in both headers, and a stored
// entry's as its compressed size too (miniz refuses the archive of a stored entry whose two sizes
// differ; an empty entry keeps compressed size 0, which miniz refuses with a claim). An entry's
// claim_end sets both sizes once the archive is packed: to the archive's size less the entry's
// local header offset and the header's 30 bytes, the largest claim miniz's central directory
// check passes (its data then runs past the archive by the length of the entry's name), or one
// more (added 2026-11-11 by orca-3mf-probe-3).
std::string pack_zip(const std::vector<ZipEntry> &entries, std::vector<std::pair<size_t, size_t>> *spans = nullptr, size_t wrong_crc = SIZE_MAX)
{
    const uint32_t date = ((2026 - 1980) << 9) | (11 << 5) | 10;
    std::string    out, central;
    // Each entry with a claim_end: the entry and the offsets of its local and central headers.
    std::vector<std::tuple<const ZipEntry *, uint32_t, size_t>> ends;
    for (const ZipEntry &e : entries) {
        const size_t   index  = size_t(&e - entries.data());
        const uint32_t crc    = uint32_t(mz_crc32(MZ_CRC32_INIT, reinterpret_cast<const mz_uint8 *>(e.data.data()), e.data.size())) ^
                             (index == wrong_crc || e.wrong_crc ? 1u : 0u);
        const uint32_t size   = e.claimed_size != 0 ? e.claimed_size : uint32_t(e.data.size());
        std::string    body   = e.data;
        uint32_t       method = 0;
        if (!e.data.empty() && !e.stored) {
            size_t len  = 0;
            void  *heap = tdefl_compress_mem_to_heap(e.data.data(), e.data.size(), &len, int(tdefl_create_comp_flags_from_zip_params(6, -15, 0)));
            if (heap == nullptr)
                throw std::runtime_error("cannot deflate " + e.name);
            body.assign(static_cast<const char *>(heap), len);
            mz_free(heap);
            method = 8;
        }
        const uint32_t offset = uint32_t(out.size());
        const uint32_t comp   = e.stored && e.claimed_size != 0 ? size : uint32_t(body.size());
        if (e.claim_end != 0)
            ends.emplace_back(&e, offset, central.size());
        put32(out, 0x04034b50);
        put16(out, 20);
        put16(out, 0);
        put16(out, method);
        put16(out, 0);
        put16(out, date);
        put32(out, crc);
        put32(out, comp);
        put32(out, size);
        put16(out, uint32_t(e.name.size()));
        put16(out, 0);
        out += e.name;
        if (spans)
            spans->emplace_back(out.size(), body.size());
        out += body;
        put32(central, 0x02014b50);
        put16(central, 20);
        put16(central, 20);
        put16(central, 0);
        put16(central, method);
        put16(central, 0);
        put16(central, date);
        put32(central, crc);
        put32(central, comp);
        put32(central, size);
        put16(central, uint32_t(e.name.size()));
        put16(central, 0);
        put16(central, 0);
        put16(central, 0);
        put16(central, 0);
        put32(central, 0);
        put32(central, offset);
        central += e.name;
    }
    const size_t archive_size = out.size() + central.size() + 22;
    for (const auto &[e, offset, at] : ends) {
        const uint32_t claim = uint32_t(archive_size - offset - 30 + (e->claim_end == 2 ? 1 : 0));
        set32(out, offset + 18, claim);
        set32(out, offset + 22, claim);
        set32(central, at + 20, claim);
        set32(central, at + 24, claim);
    }
    const uint32_t central_at = uint32_t(out.size());
    out += central;
    put32(out, 0x06054b50);
    put16(out, 0);
    put16(out, 0);
    put16(out, uint32_t(entries.size()));
    put16(out, uint32_t(entries.size()));
    put32(out, uint32_t(central.size()));
    put32(out, central_at);
    put16(out, 0);
    return out;
}

bool read_bytes(const std::filesystem::path &file, std::string &bytes)
{
    std::ifstream in(file, std::ios::binary);
    if (!in)
        return false;
    bytes.assign(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
    return true;
}

// A file's extension in lower case ASCII.
std::string lower_ext(const std::filesystem::path &file)
{
    std::string ext = file.extension().string();
    for (char &ch : ext)
        if (ch >= 'A' && ch <= 'Z')
            ch = char(ch - 'A' + 'a');
    return ext;
}

// The folder of the probe's own program (<work>/oracle/probe-build).
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

std::vector<ModelCase> generated_files()
{
    SplitMix               g(0x30DE6000u);
    std::vector<ModelCase> out;
    auto                   push = [&](const std::string &label, const std::vector<Tri> &t) {
        out.push_back({label, "gen-" + std::to_string(out.size()) + ".stl", binary_stl(t, "probe")});
    };
    const double origin[3] = {0, 0, 0}, s20[3] = {20, 20, 20}, s10[3] = {10, 10, 10};
    push("20 mm cube at the origin", box(origin, s20));
    {
        const double o[3] = {100, 100, 5}, s[3] = {30, 10, 4};
        push("a box off the origin", box(o, s));
    }
    {
        const double o[3] = {-10, -10, -30};
        push("a box below the bed", box(o, s20));
    }
    {
        const double s[3] = {300, 20, 10};
        push("a box longer than the bed", box(origin, s));
    }
    {
        const double s[3] = {10, 10, 400};
        push("a box taller than the bed", box(origin, s));
    }
    {
        const double o2[3] = {30, 5, 3};
        auto         t = box(origin, s10), t2 = box(o2, s10);
        t.insert(t.end(), t2.begin(), t2.end());
        push("two boxes in one file", t);
    }
    push("a single triangle", {Tri{{{0, 0, 0}, {10, 0, 0}, {0, 10, 0}}}});
    push("a flat square of two triangles", {Tri{{{0, 0, 0}, {10, 0, 0}, {0, 10, 0}}}, Tri{{{10, 0, 0}, {10, 10, 0}, {0, 10, 0}}}});
    push("a tetrahedron",
         {Tri{{{0, 0, 0}, {0, 10, 0}, {10, 0, 0}}}, Tri{{{0, 0, 0}, {0, 0, 10}, {0, 10, 0}}}, Tri{{{0, 0, 0}, {10, 0, 0}, {0, 0, 10}}},
          Tri{{{10, 0, 0}, {0, 10, 0}, {0, 0, 10}}}});
    out.push_back({"an ASCII box, LF", "ascii-lf.stl", ascii_stl(box(origin, s10), "\n")});
    {
        const double o[3] = {-5, 7, 1}, s[3] = {12.5, 3.25, 8};
        out.push_back({"an ASCII box, CRLF", "ascii-crlf.stl", ascii_stl(box(o, s), "\r\n")});
    }
    out.push_back({"a box in an .oltp file (256-byte header)", "box.oltp", binary_stl(box(origin, s20), "oltp", 256)});
    out.push_back({"an upper-case extension", "UPPER.STL", binary_stl(box(origin, s10), "probe")});
    out.push_back({"an empty file", "empty.stl", std::string()});
    out.push_back({"a binary header of no triangle", "no-triangles.stl", binary_stl({}, "probe")});
    {
        std::string cut = binary_stl(box(origin, s10), "probe");
        cut.resize(cut.size() - 30);
        out.push_back({"a binary file cut short", "cut.stl", cut});
    }
    out.push_back({"an unknown extension", "model.step", binary_stl(box(origin, s10), "probe")});
    out.push_back({"no extension", "model", binary_stl(box(origin, s10), "probe")});
    for (int i = 0; i < 30; ++i) {
        const double o[3] = {real(g, -150, 150), real(g, -150, 150), real(g, -40, 40)};
        const double s[3] = {real(g, 0.5, 120), real(g, 0.5, 120), real(g, 0.5, 120)};
        auto         t    = box(o, s);
        if (g.below(2)) {
            const double ax = real(g, -3.2, 3.2), ay = real(g, -3.2, 3.2), az = real(g, -3.2, 3.2);
            rotate_all(t, ax, ay, az);
        }
        if (g.below(4) == 0) {
            const double o2[3] = {o[0] + real(g, -50, 50), o[1] + real(g, -50, 50), o[2] + real(g, -20, 20)};
            const double s2[3] = {real(g, 1, 40), real(g, 1, 40), real(g, 1, 40)};
            auto         t2    = box(o2, s2);
            t.insert(t.end(), t2.begin(), t2.end());
        }
        push("generated #" + std::to_string(i), t);
    }
    return out;
}

// OrcaSlicer's test STL files (the CR box left out) and its OBJ meshes as binary STL (fan
// triangles, the normal of each in doubles rounded to floats, the header "Sketch"), from the
// oracle's source tree.
const char *const orca_objs[] = {"20mm_cube",
                                 "2x20x10",
                                 "A",
                                 "A_upsidedown",
                                 "V",
                                 "V_standing",
                                 "bridge",
                                 "cube_with_concave_hole",
                                 "cube_with_concave_hole_enlarged",
                                 "cube_with_concave_hole_enlarged_standing",
                                 "cube_with_hole",
                                 "extruder_idler",
                                 "extruder_idler_quads",
                                 "frog_legs",
                                 "ipadstand",
                                 "overhang",
                                 "pyramid",
                                 "simplification",
                                 "sloping_hole",
                                 "small_dorito",
                                 "two_hollow_squares"};

bool orca_files(std::vector<ModelCase> &out, std::string &error)
{
    std::filesystem::path exe;
    if (!program_dir(exe, error))
        return false;
    const std::filesystem::path data = exe.parent_path() / "src" / "tests" / "data";
    auto                        read = [&](const std::filesystem::path &rel, std::string &bytes) {
        if (!read_bytes(data / rel, bytes)) {
            error = "cannot read " + rel.string() + " under " + data.string();
            return false;
        }
        return true;
    };
    const struct
    {
        const char  *label;
        const char  *name;
        std::wstring rel;
    } stls[] = {
        {"orca:test_stl/ASCII/20mmbox-LF.stl", "20mmbox-LF.stl", L"test_stl/ASCII/20mmbox-LF.stl"},
        {"orca:test_stl/ASCII/20mmbox-CRLF.stl", "20mmbox-CRLF.stl", L"test_stl/ASCII/20mmbox-CRLF.stl"},
        {"orca:test_stl/ASCII/20mmbox-nonstandard.stl", "20mmbox-nonstandard.stl", L"test_stl/ASCII/20mmbox-nonstandard.stl"},
        {"orca:test_stl/Geraete/20mmbox-unicode.stl", "20mmbox-unicode.stl", L"test_stl/Geräte/20mmbox-čřšřěá.stl"},
        {"orca:test_3mf/Prusa.stl", "Prusa.stl", L"test_3mf/Prusa.stl"},
    };
    for (const auto &f : stls) {
        ModelCase c{f.label, f.name, {}};
        if (!read(f.rel, c.bytes))
            return false;
        out.push_back(c);
    }
    std::vector<bool> stl_committed;
    for (const char *o : orca_objs) {
        std::string text;
        if (!read(std::string(o) + ".obj", text))
            return false;
        std::vector<std::array<double, 3>> v;
        std::vector<Tri>                   tris;
        size_t                             pos = 0;
        while (pos < text.size()) {
            size_t end = text.find('\n', pos);
            if (end == std::string::npos)
                end = text.size();
            const std::string line = text.substr(pos, end - pos);
            pos                    = end + 1;
            if (line.size() > 2 && line[0] == 'v' && line[1] == ' ') {
                std::array<double, 3> pt{};
                const char           *s = line.c_str() + 2;
                char                 *e = nullptr;
                for (int k = 0; k < 3; ++k) {
                    pt[k] = std::strtod(s, &e);
                    s     = e;
                }
                v.push_back(pt);
            } else if (line.size() > 2 && line[0] == 'f' && line[1] == ' ') {
                std::vector<int> idx;
                const char      *s = line.c_str() + 2;
                while (*s) {
                    while (*s == ' ' || *s == '\t' || *s == '\r')
                        ++s;
                    if (!*s)
                        break;
                    char      *e = nullptr;
                    const long n = std::strtol(s, &e, 10);
                    if (e == s)
                        break;
                    idx.push_back(n < 0 ? int(v.size()) + int(n) : int(n) - 1);
                    s = e;
                    while (*s && *s != ' ' && *s != '\t' && *s != '\r')
                        ++s;
                }
                for (size_t k = 2; k < idx.size(); ++k) {
                    Tri       t;
                    const int c[3] = {idx[0], idx[k - 1], idx[k]};
                    for (int i = 0; i < 3; ++i)
                        for (int d = 0; d < 3; ++d)
                            t.v[i][d] = float(v[size_t(c[i])][d]);
                    tris.push_back(t);
                }
            }
        }
        out.push_back({std::string("orca:") + o + ".obj as model.stl", std::string(o) + ".stl", binary_stl(tris, "Sketch")});
        // The committed set takes each mesh once: as STL when that is 1500 bytes or fewer.
        out.back().full_only = out.back().bytes.size() > 1500;
        stl_committed.push_back(!out.back().full_only);
    }
    // The same OBJ meshes as they are, for the OBJ reader; committed (when of 4000 bytes or
    // fewer) where the STL is not, and the pyramid.
    for (size_t i = 0; i < std::size(orca_objs); ++i) {
        const std::string o = orca_objs[i];
        ModelCase         c{"orca:" + o + ".obj", o + ".obj", {}};
        if (!read(o + ".obj", c.bytes))
            return false;
        c.full_only = stl_committed[i] && o != "pyramid";
        out.push_back(c);
    }
    return true;
}

// A 3MF fixture: the folder model.3mf/ of its archive's entries, packed by pack_zip in the order
// of their paths; the folder's name, 3mf-<piece>-<what>, gives the piece of the port it tests. A
// file named full-only beside model.3mf/ (its text says why) keeps the fixture out of the
// committed set, for the committed file's budget. More marker files beside it change how entries
// are packed, each a line per entry ('#' starts a comment line): wrong-crc names entries packed
// with a CRC one bit off, claimed-size gives an entry's path and the uncompressed size its headers
// claim (added 2026-11-11 by orca-3mf-errors, for the reader's texts of an entry that cannot be
// extracted or read into the parser's buffer). Added 2026-11-11 by orca-3mf-probe-3, for the
// claims a reader allocates a string of before it extracts: stored names entries stored rather
// than deflated; claimed-size's size may be `end` or `past` for a stored entry (pack_zip's
// claim_end), and may be given for an empty entry (its compressed size stays 0); duplicate gives
// an entry's path and a file of the fixture's folder (outside model.3mf/), whose bytes are packed
// as another entry of that name right after it.
bool marker_lines(const std::filesystem::path &file, std::vector<std::string> &lines, std::string &error)
{
    std::error_code ec;
    if (!std::filesystem::is_regular_file(file, ec))
        return true;
    std::string text;
    if (!read_bytes(file, text)) {
        error = "cannot read " + file.string();
        return false;
    }
    std::istringstream in(text);
    for (std::string line; std::getline(in, line);) {
        if (!line.empty() && line.back() == '\r')
            line.pop_back();
        if (!line.empty() && line[0] != '#')
            lines.push_back(line);
    }
    return true;
}

bool fixture_3mf(const std::filesystem::path &d, const std::string &tag, std::vector<ModelCase> &out, std::string &error)
{
    if (tag.size() < 7 || tag.compare(0, 4, "3mf-") != 0 || tag[4] < '1' || tag[4] > '3' || tag[5] != '-') {
        error = "a 3MF fixture's folder is named 3mf-<1 to 3>-<what>, not " + d.string();
        return false;
    }
    const std::filesystem::path                                root = d / "model.3mf";
    std::error_code                                            ec;
    std::vector<std::pair<std::string, std::filesystem::path>> files;
    for (const auto &e : std::filesystem::recursive_directory_iterator(root, ec))
        if (e.is_regular_file())
            files.emplace_back(e.path().lexically_relative(root).generic_string(), e.path());
    if (ec || files.empty()) {
        error = "cannot list the entries under " + root.string();
        return false;
    }
    std::sort(files.begin(), files.end());
    ModelCase c{"fixture:" + tag, tag + ".3mf", {}};
    c.piece     = tag[4] - '0';
    c.steps     = tag == "3mf-3-bambu";
    c.full_only = std::filesystem::is_regular_file(d / "full-only", ec);
    for (const auto &f : files) {
        ZipEntry e{f.first, {}};
        if (!read_bytes(f.second, e.data)) {
            error = "cannot read " + f.second.string();
            return false;
        }
        c.entries.push_back(std::move(e));
    }
    auto entry = [&](const std::string &name) -> ZipEntry * {
        for (ZipEntry &e : c.entries)
            if (e.name == name)
                return &e;
        error = "no entry " + name + " under " + root.string();
        return nullptr;
    };
    std::vector<std::string> wrong_crc, claimed, stored, duplicate;
    if (!marker_lines(d / "wrong-crc", wrong_crc, error) || !marker_lines(d / "claimed-size", claimed, error) ||
        !marker_lines(d / "stored", stored, error) || !marker_lines(d / "duplicate", duplicate, error))
        return false;
    for (const std::string &name : wrong_crc) {
        ZipEntry *e = entry(name);
        if (e == nullptr)
            return false;
        e->wrong_crc = true;
    }
    for (const std::string &name : stored) {
        ZipEntry *e = entry(name);
        if (e == nullptr)
            return false;
        e->stored = true;
    }
    for (const std::string &line : claimed) {
        const size_t        at    = line.rfind(' ');
        ZipEntry           *e     = at == std::string::npos ? nullptr : entry(line.substr(0, at));
        const std::string   value = at == std::string::npos ? std::string() : line.substr(at + 1);
        const int           end   = value == "end" ? 1 : value == "past" ? 2 : 0;
        const unsigned long size  = end != 0 ? 0 : std::strtoul(value.c_str(), nullptr, 10);
        if (e == nullptr || (end == 0 && size == 0) || (end != 0 && !e->stored)) {
            if (error.empty())
                error = "a claimed-size line is '<entry> <size>', or '<entry> end' or '<entry> past' of a stored entry, not '" + line +
                        "' in " + d.string();
            return false;
        }
        e->claimed_size = uint32_t(size);
        e->claim_end    = end;
    }
    for (const std::string &line : duplicate) {
        const size_t at = line.rfind(' ');
        if (at == std::string::npos || entry(line.substr(0, at)) == nullptr) {
            if (error.empty())
                error = "a duplicate line is '<entry> <file>', not '" + line + "' in " + d.string();
            return false;
        }
        ZipEntry copy{line.substr(0, at), {}};
        if (!read_bytes(d / std::filesystem::u8path(line.substr(at + 1)), copy.data)) {
            error = "cannot read " + (d / std::filesystem::u8path(line.substr(at + 1))).string();
            return false;
        }
        const auto after = std::find_if(c.entries.rbegin(), c.entries.rend(), [&](const ZipEntry &e) { return e.name == copy.name; });
        c.entries.insert(after.base(), std::move(copy));
    }
    c.bytes = pack_zip(c.entries);
    out.push_back(std::move(c));
    return true;
}

// The probe's source folder (parity/oracle/probe), as the build folder's CMakeCache.txt names it
// (CMAKE_HOME_DIRECTORY): the build trims __FILE__, and the committed set's folder is not under
// the repository in a check run (parity probe --check, <work>/parity/runs/probe-check; until
// 2026-11-10 the fixtures were looked for from it, so the model probe's check could not run).
bool probe_source(std::filesystem::path &dir, std::string &error)
{
    std::filesystem::path exe;
    if (!program_dir(exe, error))
        return false;
    std::string cache;
    if (!read_bytes(exe / "CMakeCache.txt", cache)) {
        error = "cannot read " + (exe / "CMakeCache.txt").string();
        return false;
    }
    const std::string key = "\nCMAKE_HOME_DIRECTORY:INTERNAL=";
    const size_t      at  = cache.find(key);
    if (at == std::string::npos) {
        error = "no CMAKE_HOME_DIRECTORY in " + (exe / "CMakeCache.txt").string();
        return false;
    }
    std::string value = cache.substr(at + key.size(), cache.find('\n', at + key.size()) - (at + key.size()));
    if (!value.empty() && value.back() == '\r')
        value.pop_back();
    dir = std::filesystem::u8path(value);
    return true;
}

// The fixtures of parity/oracle/probe/fixtures/model/ (found from the probe's source folder,
// probe_source), a folder each, in name order: a 3MF fixture (fixture_3mf), or the file named
// model.* as the model, written under the folder's name, with the others written beside it under
// their own names.
bool fixture_files(std::vector<ModelCase> &out, std::string &error)
{
    std::filesystem::path source;
    if (!probe_source(source, error))
        return false;
    const std::filesystem::path        root = source / "fixtures" / "model";
    std::error_code                    ec;
    std::vector<std::filesystem::path> dirs;
    for (const auto &e : std::filesystem::directory_iterator(root, ec))
        if (e.is_directory())
            dirs.push_back(e.path());
    if (ec || dirs.empty()) {
        error = "cannot list the fixtures under " + root.string();
        return false;
    }
    std::sort(dirs.begin(), dirs.end());
    for (const std::filesystem::path &d : dirs) {
        const std::string tag = d.filename().string();
        if (std::filesystem::is_directory(d / "model.3mf", ec)) {
            if (!fixture_3mf(d, tag, out, error))
                return false;
            continue;
        }
        ModelCase                          c{"fixture:" + tag, {}, {}};
        std::vector<std::filesystem::path> files;
        for (const auto &e : std::filesystem::directory_iterator(d, ec))
            if (e.is_regular_file())
                files.push_back(e.path());
        std::sort(files.begin(), files.end());
        for (const std::filesystem::path &f : files) {
            std::string bytes;
            if (!read_bytes(f, bytes)) {
                error = "cannot read " + f.string();
                return false;
            }
            if (f.stem() == "model") {
                c.name  = tag + f.extension().string();
                c.bytes = std::move(bytes);
            } else
                c.extra.emplace_back(f.filename().string(), std::move(bytes));
        }
        if (c.name.empty()) {
            error = "no model.* in " + d.string();
            return false;
        }
        out.push_back(std::move(c));
    }
    return true;
}

std::string replace_all(std::string s, const std::string &from, const std::string &to)
{
    for (size_t at = s.find(from); at != std::string::npos; at = s.find(from, at + to.size()))
        s.replace(at, from.size(), to);
    return s;
}

// OBJ files made here: the line ends and bytes a text-mode read handles, lines longer than the
// reader's 64 KiB pieces, the numbers strtod and strtol read, and faces it refuses.
std::vector<ModelCase> obj_variants()
{
    using namespace std::string_literals;
    const std::string      tetra = "v 0 0 0\nv 10 0 0\nv 0 10 0\nv 0 0 10\nf 1 3 2\nf 1 2 4\nf 1 4 3\nf 2 3 4\n";
    std::vector<ModelCase> out;
    auto                   push = [&](const std::string &label, const std::string &bytes, const char *ext = ".obj") {
        out.push_back({"obj variant: " + label, "var-" + std::to_string(out.size()) + ext, bytes});
    };
    push("LF", tetra);
    push("CRLF", replace_all(tetra, "\n", "\r\n"));
    push("CR", replace_all(tetra, "\n", "\r"));
    push("no line end after the last face", tetra.substr(0, tetra.size() - 1));
    push("mixed line ends, blank lines and indents", "v 0 0 0\r\n\r\nv 10 0 0\n\rv 0 10 0\r\r\n  v 0 0 10\n f 1 3 2\n\tf 1 2 4\r\nf 1 4 3\nf 2 3 4\n");
    push("NULs end lines early", "v 0 0 0\nv 10 0 0\nv 0 10 0\0 junk\nv 0 0 10\nf 1 3 2\nf 1 2 4\nf 1 4 3\nf 2 3 4 \0 5\n"s);
    push("Ctrl-Z ends the text-mode read", replace_all(tetra, "f 2 3 4\n", "\x1a" "f 2 3 4\n"));
    push("Ctrl-Z inside a line", replace_all(tetra, "f 1 4 3\n", "f 1 4 3\x1a\nf 2 3 4\n"));
    push("a line of 65536 bytes first", "#" + std::string(65535, 'x') + "\n" + tetra);
    push("a line of 100000 bytes first", "#" + std::string(99999, 'x') + "\n" + tetra);
    push("a line of 140000 bytes after the vertices", "v 0 0 0\nv 10 0 0\nv 0 10 0\nv 0 0 10\n#" + std::string(139999, 'y') + "\nf 1 3 2\nf 1 2 4\nf 1 4 3\nf 2 3 4\n");
    push("a line of 140000 bytes last, unended", tetra + "#" + std::string(139999, 'z'));
    {
        std::string big;
        for (int i = 0; i < 3000; ++i)
            big += "v " + std::to_string(i % 17) + " " + std::to_string((i * 7) % 23) + " " + std::to_string(i / 50) + ".25\r\n";
        for (int i = 0; i + 3 <= 3000; i += 3)
            big += "f " + std::to_string(i + 1) + " " + std::to_string(i + 2) + " " + std::to_string(i + 3) + "\r\n";
        push("CRLF lines over several 64 KiB pieces", big);
    }
    push("numbers strtod reads", "v 0x1p3 0 0\nv .5 -0 5.\nv 1e-320 +3 0X1P-2\nv 0 0 1E1\nv\t2\t2\t2\nf 1 2 3\nf 1 3 4\nf 1 4 2\nf 2 4 3\nf 5 2 3\n");
    push("faces strtol reads and refuses", "v 0 0 0\nv 10 0 0\nv 0 10 0\nv 0 0 10\nf +1 3 2\nf 1/1/1/1 2 4\nf 0x1 4 3\nf 2 3 4 junk\nf 1 2 4\nf 2 3 4\n");
    push("an index past a long", "v 0 0 0\nv 10 0 0\nv 0 10 0\nf 99999999999 1 2\n");
    push("a negative index before the first vertex", "v 0 0 0\nv 10 0 0\nv 0 10 0\nf -9 1 2\n");
    push("a face of two corners", "v 0 0 0\nv 10 0 0\nv 0 10 0\nf 1 2\n");
    push("a lone f", "v 0 0 0\nv 10 0 0\nv 0 10 0\nf\nf \nf 1 2 3\n");
    push("inside out", "v 0 0 0\nv 10 0 0\nv 0 10 0\nv 0 0 10\nf 1 2 3\nf 1 4 2\nf 1 3 4\nf 2 4 3\n");
    push("an empty file", "");
    push("only comments", "# nothing\n#\n");
    push("materials without a library", "usemtl a\nv 0 0 0\nv 10 0 0\nv 0 10 0\nv 0 0 10\nf 1 3 2\nusemtl b\nf 1 2 4 3\nf 1 4 3\nf 2 3 4\n");
    push("an upper-case extension", tetra, ".OBJ");
    {
        ModelCase c{"obj variant: a material library with a line of 140000 bytes", "var-" + std::to_string(out.size()) + ".obj", "mtllib long.mtl\n" + tetra};
        c.extra.emplace_back("long.mtl", "newmtl a\n#" + std::string(139999, 'm') + "\nKd 1 0 0\n");
        out.push_back(c);
    }
    {
        ModelCase c{"obj variant: a material library of CRLF lines and Ctrl-Z", "var-" + std::to_string(out.size()) + ".obj",
                    "mtllib lib.mtl\nusemtl a\n" + tetra};
        c.extra.emplace_back("lib.mtl", "newmtl a\r\nKd 0.2 0.4 0.6\r\nKa 0.1 0.1 0.1\r\n\x1a"
                                        "newmtl b\r\n");
        out.push_back(c);
    }
    return out;
}

// The corpus's model files (<work>/parity/corpus/meshes/<case>/) but the CR box (it ends the
// engine, parity/CORPUS.md), each under its folder's name: the full set only. A 3MF goes to M3MF,
// with MLOD's steps.
bool corpus_files(std::vector<ModelCase> &out, std::string &error)
{
    std::filesystem::path exe;
    if (!program_dir(exe, error))
        return false;
    const std::filesystem::path        root = exe.parent_path().parent_path() / "parity" / "corpus" / "meshes";
    std::error_code                    ec;
    std::vector<std::filesystem::path> dirs;
    for (const auto &e : std::filesystem::directory_iterator(root, ec))
        if (e.is_directory() && e.path().filename() != "20mmbox-cr")
            dirs.push_back(e.path());
    if (ec || dirs.empty()) {
        error = "cannot list the corpus under " + root.string();
        return false;
    }
    std::sort(dirs.begin(), dirs.end());
    for (const std::filesystem::path &d : dirs) {
        std::vector<std::filesystem::path> files;
        for (const auto &e : std::filesystem::directory_iterator(d, ec))
            if (e.is_regular_file())
                files.push_back(e.path());
        std::sort(files.begin(), files.end());
        for (const std::filesystem::path &f : files) {
            const std::string tag = d.filename().string();
            ModelCase         c{"corpus:" + tag, tag + f.extension().string(), {}};
            c.full_only = true;
            if (!read_bytes(f, c.bytes)) {
                error = "cannot read a file under " + d.string();
                return false;
            }
            if (lower_ext(f) == ".3mf") {
                c.piece = 3;
                c.steps = true;
            }
            out.push_back(c);
        }
    }
    return true;
}

// OrcaSlicer's own 3MF projects (its calibration and handy models, from the oracle's source tree,
// <exe>/../src/resources), in the order of their paths, each under its own name: the full set
// only, with MLOD's steps.
bool resource_3mfs(std::vector<ModelCase> &out, std::string &error)
{
    std::filesystem::path exe;
    if (!program_dir(exe, error))
        return false;
    const std::filesystem::path                                root = exe.parent_path() / "src" / "resources";
    std::error_code                                            ec;
    std::vector<std::pair<std::string, std::filesystem::path>> files;
    for (const auto &e : std::filesystem::recursive_directory_iterator(root, ec))
        if (e.is_regular_file() && lower_ext(e.path()) == ".3mf")
            files.emplace_back(e.path().lexically_relative(root).generic_string(), e.path());
    if (ec || files.empty()) {
        error = "cannot list the 3MF files under " + root.string();
        return false;
    }
    std::sort(files.begin(), files.end());
    for (const auto &f : files) {
        ModelCase c{"orca:resources/" + f.first, f.second.filename().string(), {}};
        c.full_only = true;
        c.piece     = 3;
        c.steps     = true;
        if (!read_bytes(f.second, c.bytes)) {
            error = "cannot read " + f.second.string();
            return false;
        }
        out.push_back(std::move(c));
    }
    return true;
}

// Archives the reader refuses, made from the plain fixture's entries: not an archive, an empty
// file, the archive cut in half, without its end of central directory record, the root model's
// CRC one bit off, and its deflate stream with a byte changed. The cut ones log and throw what
// the first does (miniz cannot open them): the full set only, for the committed file's budget.
bool broken_archives(const std::vector<ModelCase> &fixtures, std::vector<ModelCase> &out, std::string &error)
{
    const ModelCase *plain = nullptr;
    for (const ModelCase &c : fixtures)
        if (c.label == "fixture:3mf-3-plain")
            plain = &c;
    size_t model_at = 0;
    while (plain != nullptr && model_at < plain->entries.size() && plain->entries[model_at].name != "3D/3dmodel.model")
        ++model_at;
    if (plain == nullptr || model_at == plain->entries.size()) {
        error = "no fixture 3mf-3-plain with a 3D/3dmodel.model";
        return false;
    }
    auto push = [&](const std::string &label, std::string bytes, bool full_only = false) {
        ModelCase c{"broken archive: " + label, "broken-" + std::to_string(out.size()) + ".3mf", std::move(bytes)};
        c.piece     = 1;
        c.full_only = full_only;
        out.push_back(std::move(c));
    };
    push("not an archive", "this is not a zip archive\n");
    push("an empty file", std::string());
    push("cut in half", plain->bytes.substr(0, plain->bytes.size() / 2), true);
    push("no end of central directory record", plain->bytes.substr(0, plain->bytes.size() - 22), true);
    push("the root model's CRC one bit off", pack_zip(plain->entries, nullptr, model_at));
    {
        std::vector<std::pair<size_t, size_t>> spans;
        std::string                            s = pack_zip(plain->entries, &spans);
        s[spans[model_at].first + spans[model_at].second / 2] ^= 0x55;
        push("a deflate stream with a byte changed", s);
    }
    return true;
}

// Variants of the 3mf-2-numbers fixture, each with one number of its height ranges or cut
// information in another form: the forms num_get (through property_tree's stream_translator)
// refuses, and those at the edge of what it takes (hexadecimal, infinities, numbers past a type's
// range, denormals, white space as character references). Each is piece 2 and named
// numbers-<n>.3mf; the full set only, but those marked committed.
bool number_variants(const std::vector<ModelCase> &fixtures, std::vector<ModelCase> &out, std::string &error)
{
    const ModelCase *numbers = nullptr;
    for (const ModelCase &c : fixtures)
        if (c.label == "fixture:3mf-2-numbers")
            numbers = &c;
    if (numbers == nullptr) {
        error = "no fixture 3mf-2-numbers";
        return false;
    }
    struct Variant
    {
        const char *entry, *from, *to;
        bool        committed;
    };
    const char *const ranges = "Metadata/layer_config_ranges.xml", *const cut = "Metadata/cut_information.xml";
    const Variant     variants[] = {
        {ranges, "min_z=\" 0.5\"", "min_z=\"0x1p-1\"", false},
        {ranges, "min_z=\" 0.5\"", "min_z=\"inf\"", false},
        {ranges, "min_z=\" 0.5\"", "min_z=\"nan\"", false},
        {ranges, "min_z=\" 0.5\"", "min_z=\"1e999\"", false},
        {ranges, "min_z=\" 0.5\"", "min_z=\"1e-999\"", false},
        {ranges, "min_z=\" 0.5\"", "min_z=\"4.9e-324\"", false},
        {ranges, "min_z=\" 0.5\"", "min_z=\"\"", false},
        {ranges, "min_z=\" 0.5\"", "min_z=\"  \"", false},
        {ranges, "min_z=\" 0.5\"", "min_z=\"0,5\"", false},
        {ranges, "min_z=\" 0.5\"", "min_z=\"0.5x\"", false},
        {ranges, "min_z=\" 0.5\"", "min_z=\"- 0.5\"", false},
        {ranges, "min_z=\" 0.5\"", "min_z=\"--0.5\"", false},
        {ranges, "min_z=\" 0.5\"", "min_z=\"5e\"", false},
        {ranges, "min_z=\" 0.5\"", "min_z=\".\"", false},
        {ranges, "min_z=\" 0.5\"", "min_z=\"+.5\"", false},
        {ranges, "min_z=\" 0.5\"", "min_z=\"0.5 0.6\"", false},
        {ranges, "min_z=\" 0.5\"", "min_z=\"&#9;0.5&#10;\"", false},
        {ranges, "id=\" 1 \"", "id=\"2147483648\"", false},
        {ranges, "id=\" 1 \"", "id=\"01\"", false},
        {cut, "check_sum=\"+1\"", "check_sum=\"-1\"", false},
        {cut, "check_sum=\"+1\"", "check_sum=\"18446744073709551616\"", false},
        {cut, "check_sum=\"+1\"", "check_sum=\"0x10\"", false},
        {cut, "volume_id=\" 0\"", "volume_id=\"2147483648\"", true},
        {cut, "r_tolerance=\".5\"", "r_tolerance=\"1e39\"", true},
        {cut, "r_tolerance=\".5\"", "r_tolerance=\"1e-46\"", false},
    };
    for (const Variant &v : variants) {
        std::vector<ZipEntry> entries = numbers->entries;
        const auto            e  = std::find_if(entries.begin(), entries.end(), [&](const ZipEntry &z) { return z.name == v.entry; });
        const size_t          at = e == entries.end() ? std::string::npos : e->data.find(v.from);
        if (at == std::string::npos || e->data.find(v.from, at + 1) != std::string::npos) {
            error = std::string("the fixture 3mf-2-numbers has no single ") + v.from + " in " + v.entry;
            return false;
        }
        e->data.replace(at, std::strlen(v.from), v.to);
        ModelCase c{std::string("3mf-2-numbers with ") + v.to, "numbers-" + std::to_string(out.size()) + ".3mf", pack_zip(entries)};
        c.piece     = 2;
        c.full_only = !v.committed;
        out.push_back(std::move(c));
    }
    return true;
}

// ---- MLOD ----------------------------------------------------------------------------------

struct Pair
{
    std::string key, value;
};

std::vector<Pair> dump_pairs(const DynamicPrintConfig &c)
{
    std::vector<Pair> out;
    for (const std::string &k : c.keys())
        out.push_back({k, c.opt_serialize(k)});
    return out;
}

std::vector<Pair> arrange_config(SplitMix &g)
{
    DynamicPrintConfig c;
    c.set_key_value("curr_bed_type", new ConfigOptionEnum<BedType>(BedType(g.below(7))));
    c.set_key_value("hot_plate_temp", new ConfigOptionInts({55, 60, 65}));
    c.set_key_value("hot_plate_temp_initial_layer", new ConfigOptionInts({60, 65}));
    c.set_key_value("textured_plate_temp", new ConfigOptionInts({50}));
    c.set_key_value("cool_plate_temp", new ConfigOptionInts({35, 40}));
    c.set_key_value("eng_plate_temp_initial_layer", new ConfigOptionInts({70}));
    c.set_key_value("nozzle_temperature", new ConfigOptionInts({220, 250, 210}));
    c.set_key_value("nozzle_temperature_initial_layer", new ConfigOptionInts({225, 255}));
    if (g.below(2))
        c.set_key_value("temperature_vitrification", new ConfigOptionInts({45, 70}));
    const char *const types[] = {"PLA", "PETG", "ABS", "TPU", "PC", "Generic", "PA-CF"};
    const char       *t0 = types[g.below(7)], *t1 = types[g.below(7)], *t2 = types[g.below(7)];
    c.set_key_value("filament_type", new ConfigOptionStrings({t0, t1, t2}));
    c.set_key_value("enable_support", new ConfigOptionBool(g.below(2) != 0));
    c.set_key_value("support_type", new ConfigOptionEnum<SupportType>(SupportType(g.below(4))));
    c.set_key_value("support_filament", new ConfigOptionInt(int(g.below(3))));
    c.set_key_value("support_interface_filament", new ConfigOptionInt(int(g.below(4))));
    return dump_pairs(c);
}

void put_ap(Payload &p, const arrangement::ArrangePolygon &ap)
{
    put_points(p, ap.poly.contour.points);
    p.i64(ap.translation.x()).i64(ap.translation.y()).f64(ap.rotation).i32(ap.bed_idx);
    p.u64(ap.extrude_ids.size());
    for (int e : ap.extrude_ids)
        p.i32(e);
    p.i32(ap.bed_temp).i32(ap.first_bed_temp).i32(ap.print_temp).i32(ap.first_print_temp).i32(ap.vitrify_temp).i32(ap.filament_temp_type);
    p.f64(ap.brim_width).boolean(ap.has_tree_support).f64(ap.height).str(ap.name).i64(ap.inflation);
}

void put_model(Payload &p, const Model &model)
{
    p.u64(model.objects.size());
    for (const ModelObject *o : model.objects) {
        p.str(o->name).boolean(o->printable);
        put_vec3(p, o->origin_translation);
        p.str(config_dump(o->config));
        p.u64(o->instances.size());
        for (const ModelInstance *i : o->instances) {
            put_mat(p, i->get_matrix());
            p.u8(uint8_t(i->print_volume_state)).boolean(i->printable).boolean(i->auto_drop).i32(i->arrange_order);
        }
        p.u64(o->volumes.size());
        for (const ModelVolume *v : o->volumes) {
            p.str(v->name).i32(int(v->type()));
            put_mat(p, v->get_matrix());
            p.u64(v->mesh().its.vertices.size()).u64(v->mesh().its.indices.size()).u64(its_hash(v->mesh().its));
            const std::shared_ptr<const TriangleMesh> &hull = v->get_convex_hull_shared_ptr();
            p.boolean(hull != nullptr);
            if (hull)
                p.u64(hull->its.vertices.size()).u64(hull->its.indices.size()).u64(its_hash(hull->its));
            p.i32(v->source.object_idx).i32(v->source.volume_idx);
            put_vec3(p, v->source.mesh_offset);
            p.str(config_dump(v->config));
            p.i32(v->get_repaired_errors_count());
            // Its painting: supports, seams, colours and fuzzy skin.
            put_facets(p, v->supported_facets);
            put_facets(p, v->seam_facets);
            put_facets(p, v->mmu_segmentation_facets);
            put_facets(p, v->fuzzy_skin_facets);
        }
    }
}

// The inputs of the steps after loading: two instances (offset, scale, rotation, mirror),
// allow_negative_z, the beds, the print settings and three arrange results.
struct LodInputs
{
    double              inst[2][12] = {};
    bool                allow_negative_z = false;
    std::vector<size_t> bed_ids;
    std::vector<Pair>   cfg;
    struct Placed
    {
        int32_t bed;
        int64_t x, y;
        double  rot;
        int32_t item;
    } placed[3] = {};
};

// Draws the inputs from g and writes them to p.
LodInputs lod_inputs(Payload &p, SplitMix &g)
{
    LodInputs in;
    for (auto &row : in.inst) {
        for (int k = 0; k < 3; ++k)
            row[k] = real(g, -150, 150);
        for (int k = 3; k < 6; ++k)
            row[k] = real(g, 0.2, 3);
        for (int k = 6; k < 9; ++k)
            row[k] = g.below(3) == 0 ? 0. : real(g, -3.2, 3.2);
        for (int k = 9; k < 12; ++k)
            row[k] = g.below(5) == 0 ? -1. : 1.;
        for (double d : row)
            p.f64(d);
    }
    in.allow_negative_z = g.below(2) != 0;
    p.boolean(in.allow_negative_z);
    // The beds, by their index in beds(): MBVL's records hold them, in that order.
    in.bed_ids = {0, 1, 2, 3, 4};
    p.u64(in.bed_ids.size());
    for (size_t b : in.bed_ids)
        p.u64(b);
    in.cfg = arrange_config(g);
    p.u64(in.cfg.size());
    for (const Pair &kv : in.cfg)
        p.str(kv.key).str(kv.value);
    for (LodInputs::Placed &pl : in.placed) {
        pl.bed  = g.below(4) == 0 ? -1 : int32_t(g.below(2));
        pl.x    = int64_t(g.below(300000000)) - 50000000;
        pl.y    = int64_t(g.below(300000000)) - 50000000;
        pl.rot  = real(g, -3.2, 3.2);
        pl.item = int32_t(g.below(10));
        p.i32(pl.bed).i64(pl.x).i64(pl.y).f64(pl.rot).i32(pl.item);
    }
    return in;
}

void write_file(const std::filesystem::path &to, const std::string &bytes)
{
    std::ofstream out(to, std::ios::binary);
    out.write(bytes.data(), std::streamsize(bytes.size()));
    if (!out)
        throw std::runtime_error("cannot write the temporary file");
}

// cli/run.cpp's load_model after the read: each configuration cut to "extruder", the height ranges
// and the layer height profile dropped, the first plate, ensure_on_bed.
void cli_cuts(Model &model)
{
    for (ModelObject *mo : model.objects) {
        for (const std::string &key : mo->config.keys())
            if (key != "extruder")
                mo->config.erase(key);
        mo->layer_config_ranges.clear();
        mo->layer_height_profile.clear();
        for (ModelVolume *v : mo->volumes)
            for (const std::string &key : v->config.keys())
                if (key != "extruder")
                    v->config.erase(key);
    }
    model.curr_plate_index = 0;
    for (ModelObject *mo : model.objects)
        mo->ensure_on_bed();
}

// The steps after loading, on the first object: its boxes, hulls and z, two instances added,
// ensure_on_bed, the print volume states, the arrange polygons of its first three instances with a
// result applied to each, and center_around_origin. They run only when every part has a hull (and,
// when `require_parts`, the object has a part).
void lod_tail(Payload &p, Model &model, const LodInputs &in, bool require_parts)
{
    // Past here the command line has refused a part of fewer than 4 facets or a flat box
    // (check_solid_objects); a part without a hull would be dereferenced.
    ModelObject *o       = model.objects.front();
    bool         hull_ok = !require_parts || !o->volumes.empty();
    for (const ModelVolume *v : o->volumes)
        hull_ok = hull_ok && v->get_convex_hull_shared_ptr() != nullptr;
    p.boolean(hull_ok);
    if (!hull_ok)
        return;
    const auto &inst             = in.inst;
    const bool  allow_negative_z = in.allow_negative_z;
    const auto &bed_ids          = in.bed_ids;
    const auto &cfg              = in.cfg;
    const auto &placed           = in.placed;
    const auto  all_beds         = beds();
    put_bb3(p, o->bounding_box_approx());
    put_bb3(p, o->bounding_box_exact());
    p.f64(o->min_z()).f64(o->max_z());
    put_bb3(p, o->instance_bounding_box(0, false));
    put_bb3(p, o->instance_bounding_box(0, true));
    put_bb3(p, o->instance_convex_hull_bounding_box(size_t(0), false));
    put_bb3(p, o->instance_convex_hull_bounding_box(size_t(0), true));
    put_bb3(p, o->raw_bounding_box());
    put_bb3(p, o->raw_mesh_bounding_box());
    put_bb3(p, o->full_raw_mesh_bounding_box());
    p.u64(o->facets_count()).u64(o->parts_count());
    p.f64(o->get_instance_min_z(0)).f64(o->get_instance_max_z(0));
    put_points(p, o->instances[0]->convex_hull_2d().points);
    for (const auto &row : inst)
        o->add_instance(Vec3d(row[0], row[1], row[2]), Vec3d(row[3], row[4], row[5]), Vec3d(row[6], row[7], row[8]), Vec3d(row[9], row[10], row[11]));
    for (size_t i = 0; i < o->instances.size(); ++i) {
        put_mat(p, o->instances[i]->get_matrix());
        put_bb3(p, o->instance_bounding_box(i, false));
        p.f64(o->get_instance_min_z(i)).f64(o->get_instance_max_z(i));
        put_points(p, o->instances[i]->convex_hull_2d().points);
    }
    put_bb3(p, o->bounding_box_exact());
    o->ensure_on_bed(allow_negative_z);
    for (const ModelInstance *i : o->instances)
        put_vec3(p, i->get_offset());
    p.f64(o->min_z());
    for (size_t b : bed_ids) {
        const Bed  &bed = all_beds[b];
        BuildVolume bv(bed.area, bed.height, bed.extruders, bed.extruder_heights);
        p.u32(model.update_print_volume_state(bv));
        for (const ModelInstance *i : o->instances)
            p.u8(uint8_t(i->print_volume_state));
    }
    DynamicPrintConfig config;
    for (const Pair &kv : cfg)
        config.set_deserialize_strict(kv.key, kv.value);
    for (size_t i = 0; i < o->instances.size() && i < std::size(placed); ++i) {
        arrangement::ArrangePolygon ap = get_instance_arrange_poly(o->instances[i], config);
        put_ap(p, ap);
        ap.bed_idx     = placed[i].bed;
        ap.translation = Point(placed[i].x, placed[i].y);
        ap.rotation    = placed[i].rot;
        ap.itemid      = placed[i].item;
        ap.apply();
        ap.apply();
        put_mat(p, o->instances[i]->get_matrix());
        p.i32(o->instances[i]->arrange_order).i32(ap.is_applied);
    }
    put_bb3(p, o->bounding_box_exact());
    o->center_around_origin(false);
    put_vec3(p, o->origin_translation);
    for (const ModelVolume *v : o->volumes)
        put_vec3(p, v->get_offset());
    put_bb3(p, o->bounding_box_exact());
}

void record_lod(Sets &sets, const std::filesystem::path &dir, const ModelCase &c, SplitMix &g, bool committed)
{
    Payload p;
    p.str(c.label).str(c.name).str(c.bytes);
    p.u64(c.extra.size());
    for (const auto &x : c.extra)
        p.str(x.first).str(x.second);
    const LodInputs in = lod_inputs(p, g);

    const std::filesystem::path file = dir / c.name;
    write_file(file, c.bytes);
    for (const auto &x : c.extra)
        write_file(dir / x.first, x.second);
    Model       model;
    std::string error;
    try {
        DynamicPrintConfig        config;
        ConfigSubstitutionContext substitutions(ForwardCompatibilitySubstitutionRule::EnableSilent);
        model = Model::read_from_file(file.string(), &config, &substitutions, LoadStrategy::LoadModel | LoadStrategy::AddDefaultInstances);
    } catch (const std::exception &e) {
        error = e.what();
        if (error.empty())
            error = "unknown error";
    }
    std::error_code ec;
    std::filesystem::remove(file, ec);
    for (const auto &x : c.extra)
        std::filesystem::remove(dir / x.first, ec);
    if (!error.empty()) {
        p.u8(1).str(error);
        add(sets, "MLOD", p, committed);
        return;
    }
    p.u8(0);
    cli_cuts(model);
    put_model(p, model);
    lod_tail(p, model, in, false);
    add(sets, "MLOD", p, committed);
}

// ---- M3MF ----------------------------------------------------------------------------------

// s with the temporary folder written <dir>, in either separator form.
std::string portable(std::string s, const std::filesystem::path &dir)
{
    s = replace_all(s, dir.string(), "<dir>");
    return replace_all(s, dir.generic_string(), "<dir>");
}

// The engine's log of one read, as the command line prints it on stderr (cli/main.cpp,
// log_to_stderr, at its default level 1): the Boost.Log records at error and fatal, each
// "[<severity>] <message>" and a line feed unless the message ends with one. The importer's
// add_error texts reach it through log_errors (bbs_3mf.cpp, load_bbs_3mf), each at error. While
// this sink is held Boost.Log's default sink (stdout, every level the core lets through) is off.
class ReadLog
{
public:
    ReadLog()
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
    ReadLog(const ReadLog &)            = delete;
    ReadLog &operator=(const ReadLog &) = delete;
    ~ReadLog() { boost::log::core::get()->remove_sink(sink_); }
    std::string text()
    {
        sink_->flush();
        return text_.str();
    }

private:
    std::ostringstream                                                                               text_;
    boost::shared_ptr<boost::log::sinks::synchronous_sink<boost::log::sinks::text_ostream_backend>> sink_;
};

// The model's information as the root model's metadata leave it (bbs_3mf.cpp, _handle_end_metadata
// and load_model_from_file; each value through xml_unescape after expat's own unescaping): the
// design information (set only when a Designer is given), the profile information and the model
// information with every metadata item by name. The exporter writes it into a 3MF again.
void put_info(Payload &p, const Model &model)
{
    p.boolean(model.design_info != nullptr);
    if (model.design_info)
        p.str(model.design_info->DesignId).str(model.design_info->Designer).str(model.design_info->DesignerUserId);
    p.boolean(model.profile_info != nullptr);
    if (model.profile_info) {
        const ModelProfileInfo &i = *model.profile_info;
        p.str(i.ProfileTile).str(i.ProfileCover).str(i.ProfileDescription).str(i.ProfileUserId).str(i.ProfileUserName);
    }
    p.boolean(model.model_info != nullptr);
    if (model.model_info) {
        const ModelInfo &i = *model.model_info;
        p.str(i.cover_file).str(i.license).str(i.description).str(i.copyright).str(i.model_name).str(i.origin);
        p.u64(i.metadata_items.size());
        for (const auto &kv : i.metadata_items)
            p.str(kv.first).str(kv.second);
    }
}

// What read_from_file leaves that the command line's cuts drop or put_model leaves out: the
// model's information (put_info); each object's module, whole configuration, layer height
// profile, height ranges and brim ears; each instance's loaded id and assembly transformation
// (when it has one); each part's whole configuration. A part's source (file, units,
// transformation) only the GUI reads: not recorded.
void put_as_read(Payload &p, Model &model)
{
    put_info(p, model);
    p.u64(model.objects.size());
    for (ModelObject *o : model.objects) {
        p.str(o->module_name).str(config_dump(o->config));
        const std::vector<coordf_t> profile = o->layer_height_profile.get();
        p.u64(profile.size());
        for (double z : profile)
            p.f64(z);
        p.u64(o->layer_config_ranges.size());
        for (const auto &r : o->layer_config_ranges)
            p.f64(r.first.first).f64(r.first.second).str(config_dump(r.second));
        p.u64(o->brim_points.size());
        for (const BrimPoint &b : o->brim_points)
            p.f32(b.pos.x()).f32(b.pos.y()).f32(b.pos.z()).f32(b.head_front_radius);
        p.u64(o->instances.size());
        for (ModelInstance *i : o->instances) {
            p.u64(i->loaded_id).boolean(i->is_assemble_initialized());
            if (i->is_assemble_initialized()) {
                put_mat(p, i->get_assemble_transformation().get_matrix());
                put_vec3(p, i->get_offset_to_assembly());
            }
        }
        p.u64(o->volumes.size());
        for (const ModelVolume *v : o->volumes)
            p.str(config_dump(v->config));
    }
}

void record_3mf(Sets &sets, const std::filesystem::path &dir, const ModelCase &c, SplitMix &g, bool committed)
{
    Payload body;
    body.str(c.name).str(c.bytes).boolean(c.steps);
    LodInputs in;
    if (c.steps)
        in = lod_inputs(body, g);

    const std::filesystem::path file = dir / c.name;
    write_file(file, c.bytes);
    Model       model;
    std::string error, log;
    {
        // As cli/run.cpp's load_model asks: the plate data, presets, BBL flag and version are
        // released unread.
        DynamicPrintConfig        config;
        ConfigSubstitutionContext substitutions(ForwardCompatibilitySubstitutionRule::EnableSilent);
        PlateDataPtrs             plates;
        std::vector<Preset *>     presets;
        bool                      is_bbl_3mf = false;
        Semver                    version;
        ReadLog                   read_log;
        try {
            model = Model::read_from_file(file.string(), &config, &substitutions, LoadStrategy::LoadModel | LoadStrategy::AddDefaultInstances, &plates,
                                          &presets, &is_bbl_3mf, &version);
        } catch (const std::exception &e) {
            error = e.what();
            if (error.empty())
                error = "unknown error";
        }
        log = read_log.text();
        release_PlateData_list(plates);
        for (Preset *preset : presets)
            delete preset;
    }
    std::error_code ec;
    std::filesystem::remove(file, ec);
    Payload p;
    p.str(c.label).u8(uint8_t(error.empty() ? 3 : c.piece));
    p.bytes += body.bytes;
    p.str(portable(log, dir));
    if (!error.empty()) {
        p.u8(1).str(portable(error, dir));
        add(sets, "M3MF", p, committed);
        return;
    }
    p.u8(0);
    put_as_read(p, model);
    cli_cuts(model);
    put_model(p, model);
    if (c.steps)
        lod_tail(p, model, in, true);
    add(sets, "M3MF", p, committed);
}

// ---- MARP ----------------------------------------------------------------------------------

void record_arp(Sets &sets)
{
    SplitMix g(0x30DE7000u);
    for (int i = 0; i < 60; ++i) {
        Payload            p;
        DynamicPrintConfig c;
        const int          bed = int(g.below(4));
        std::vector<Vec2d> area;
        if (bed == 0)
            area = rect(0, 0, 256, 256);
        else if (bed == 1)
            area = {{0, 0}, {0, 300}, {320, 300}, {320, 0}};
        else if (bed == 2)
            area = round_bed(72, 100, 0, 0, 0.01, 100);
        else
            area = rect(-90, -90, 90, 90);
        c.set_key_value("printable_area", new ConfigOptionPoints(Pointfs(area.begin(), area.end())));
        if (g.below(5) == 0)
            c.set_key_value("extruder_printable_area", new ConfigOptionPointsGroups({rect(0, 0, 230, 256), rect(20, 0, 256, 256)}));
        c.set_key_value("skirt_loops", new ConfigOptionInt(int(g.below(3))));
        c.set_key_value("skirt_height", new ConfigOptionInt(int(g.below(3))));
        c.set_key_value("draft_shield", new ConfigOptionEnum<DraftShield>(g.below(4) == 0 ? dsEnabled : dsDisabled));
        c.set_key_value("skirt_distance", new ConfigOptionFloat(real(g, 0, 10)));
        const double first_width = g.below(3) == 0 ? 0. : real(g, 0.3, 0.8);
        const bool   first_pct   = g.below(5) == 0;
        c.set_key_value("initial_layer_line_width", new ConfigOptionFloatOrPercent(first_width, first_pct));
        const double width = g.below(3) == 0 ? 0. : real(g, 0.3, 0.8);
        c.set_key_value("line_width", new ConfigOptionFloatOrPercent(width, false));
        c.set_key_value("nozzle_diameter", new ConfigOptionFloats({0.4, real(g, 0.2, 0.8)}));
        const std::vector<Pair> cfg = dump_pairs(c);
        p.u64(cfg.size());
        for (const Pair &kv : cfg)
            p.str(kv.key).str(kv.value);
        DynamicPrintConfig config;
        for (const Pair &kv : cfg)
            config.set_deserialize_strict(kv.key, kv.value);

        arrangement::ArrangeParams params;
        params.is_seq_print        = g.below(3) == 0;
        params.bed_shrink_x        = float(real(g, 0, 5));
        params.bed_shrink_y        = float(real(g, 0, 5));
        params.clearance_radius    = float(real(g, 0, 80));
        params.min_obj_distance    = g.below(2) ? 0 : coord_t(g.below(20000000));
        params.nozzle_height       = float(real(g, 0, 50));
        params.object_skirt_offset = float(real(g, 0, 6));
        p.boolean(params.is_seq_print).f32(params.bed_shrink_x).f32(params.bed_shrink_y).f32(params.clearance_radius).i64(params.min_obj_distance);
        p.f32(params.nozzle_height).f32(params.object_skirt_offset);
        arrangement::ArrangePolygons selected, unselected;
        const int                    ns = int(g.below(5)), nu = int(g.below(4));
        auto                         poly = [&](arrangement::ArrangePolygon &ap) {
            const double w = real(g, 1, 300), h = real(g, 1, 300), x = real(g, -50, 100), y = real(g, -50, 100);
            ap.poly.contour = Polygon(Points{Point(scaled(x), scaled(y)), Point(scaled(x + w), scaled(y)), Point(scaled(x + w), scaled(y + h)),
                                             Point(scaled(x), scaled(y + h))});
        };
        const double brims[] = {1., 6., 24., 3.5};
        p.u64(size_t(ns));
        for (int k = 0; k < ns; ++k) {
            arrangement::ArrangePolygon ap;
            poly(ap);
            ap.height           = real(g, 0, 80);
            ap.brim_width       = brims[g.below(4)];
            ap.has_tree_support = g.below(4) == 0;
            put_points(p, ap.poly.contour.points);
            p.f64(ap.height).f64(ap.brim_width).boolean(ap.has_tree_support);
            selected.push_back(ap);
        }
        p.u64(size_t(nu));
        for (int k = 0; k < nu; ++k) {
            arrangement::ArrangePolygon ap;
            poly(ap);
            ap.brim_width               = real(g, 0, 10);
            ap.is_virt_object           = g.below(2) != 0;
            ap.is_extrusion_cali_object = g.below(3) == 0;
            put_points(p, ap.poly.contour.points);
            p.f64(ap.brim_width).boolean(ap.is_virt_object).boolean(ap.is_extrusion_cali_object);
            unselected.push_back(ap);
        }
        put_points(p, get_bed_shape(config));
        put_points(p, get_bed_shape(config, false));
        arrangement::update_arrange_params(params, &config, selected);
        p.f32(params.brim_skirt_distance).f32(params.bed_shrink_x).f32(params.bed_shrink_y);
        put_points(p, arrangement::get_shrink_bedpts(&config, params));
        arrangement::update_selected_items_inflation(selected, &config, params);
        p.i64(params.min_obj_distance);
        for (const auto &ap : selected)
            p.i64(ap.inflation);
        arrangement::update_unselected_items_inflation(unselected, &config, params);
        for (const auto &ap : unselected) {
            p.i64(ap.inflation);
            put_points(p, ap.poly.contour.points);
        }
        add(sets, "MARP", p, i < 20);
    }
}

// ---- MTMP ----------------------------------------------------------------------------------

void record_tmp(Sets &sets)
{
    const char *const names[] = {"PLA", "PETG", "ABS",     "ASA", "TPU",    "PA",       "PA-CF", "PC",  "PVA", "HIPS", "PET",
                                 "PCTG", "PEEK", "PPS",     "FLEX", "pla",   "PLA ",     "Generic", "",  "PA6-GF", "PLA-AERO", "PHA",
                                 "SBS", "TPI",  "PE",      "PP",  "POM",    "PVDF",     "BVOH",  "CoPE", "EVA", "PCL", "PI"};
    Payload           p;
    p.u64(std::size(names));
    for (const char *n : names)
        p.str(n).i32(int(Print::get_filament_temp_type(n)));
    const std::vector<std::vector<int>> sets_of = {{}, {0}, {1}, {2}, {3}, {0, 1}, {0, 2}, {1, 2}, {1, 3}, {0, 1, 2, 3}};
    p.u64(sets_of.size());
    for (const auto &s : sets_of) {
        p.i32s(s);
        p.i32(Print::get_compatible_filament_type(std::set<int>(s.begin(), s.end())));
    }
    add(sets, "MTMP", p, true);
}

bool run_model(const Options &options, std::string &error)
{
    tbb::global_control gc(tbb::global_control::max_allowed_parallelism, 1);
    tbb::task_arena     arena{8, 8u};
    bool                ok = true;
    Sets                sets;
    head(sets, "model");
    std::error_code             ec;
    const std::filesystem::path dir = std::filesystem::temp_directory_path(ec) / ("sketch-orca-probe-model-" + std::to_string(_getpid()));
    if (ec || !std::filesystem::create_directories(dir, ec)) {
        error = "cannot make a temporary folder";
        return false;
    }
    set_temporary_dir((dir / "backup").string());
    arena.execute([&]() {
        try {
            record_xfm(sets);
            record_fac(sets);
            record_cfg(sets);
            record_cir(sets);
            record_bvl(sets);
            std::vector<ModelCase> files = generated_files();
            std::vector<ModelCase> more;
            if (!(ok = orca_files(more, error) && fixture_files(more, error)))
                return;
            for (ModelCase &c : obj_variants())
                more.push_back(std::move(c));
            if (!(ok = corpus_files(more, error)))
                return;
            // The 3MF files go to M3MF: the fixtures, the refused archives, the corpus's and
            // OrcaSlicer's own.
            std::vector<ModelCase> three;
            for (ModelCase &c : more)
                if (c.piece != 0)
                    three.push_back(c);
            more.erase(std::remove_if(more.begin(), more.end(), [](const ModelCase &c) { return c.piece != 0; }), more.end());
            std::vector<ModelCase> broken, numbers;
            if (!(ok = broken_archives(three, broken, error) && number_variants(three, numbers, error) && resource_3mfs(three, error)))
                return;
            // After the fixtures: the refused archives, then the number variants.
            broken.insert(broken.end(), numbers.begin(), numbers.end());
            const auto made = std::find_if(three.begin(), three.end(), [](const ModelCase &c) { return c.label.rfind("fixture:", 0) != 0; });
            three.insert(made, broken.begin(), broken.end());
            files.insert(files.end(), more.begin(), more.end());
            SplitMix g(0x30DE8000u);
            int generated = 0;
            for (const ModelCase &c : files) {
                // The committed set: the files of 4000 bytes or fewer (with the files beside
                // them), of the random boxes the first eight; the corpus's in the full set only.
                const bool random = c.label.rfind("generated #", 0) == 0;
                size_t     size   = c.bytes.size();
                for (const auto &x : c.extra)
                    size += x.second.size();
                record_lod(sets, dir, c, g, !c.full_only && size <= 4000 && (!random || generated < 8));
                generated += random ? 1 : 0;
            }
            SplitMix g3(0x30DE9000u);
            for (const ModelCase &c : three)
                record_3mf(sets, dir, c, g3, !c.full_only && c.bytes.size() <= 4000);
            record_arp(sets);
            record_tmp(sets);
        } catch (const std::exception &e) {
            ok    = false;
            error = e.what();
        }
    });
    std::filesystem::remove_all(dir, ec);
    if (!ok)
        return false;
    return save(sets, options, "model.sop1", error);
}

} // namespace

const Target &target()
{
    static const Target t{"model", false, run_model};
    return t;
}

} // namespace probe
