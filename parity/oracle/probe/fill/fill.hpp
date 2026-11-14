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
// What the fill probe's sources share (fill.sop1, parity/sop1/fill.md; orca-o3-probe-fill,
// 2026-11-13): the values the page names (point, expolygon, surface, polyline, thick polyline,
// box, flow, params, filler, configs, entity), the seeded generator's helpers, the shapes the
// probe builds itself and the sections of OrcaSlicer's test meshes, the committed-set rule, the
// capture of the engine's stdout and error log around a call, and FillAccess, which reaches
// Fill's protected members through a pointer to member named by a derived class (no engine code
// is copied).
#pragma once

#include "../probe.hpp"

#include <cstdint>
#include <exception>
#include <functional>
#include <sstream>
#include <string>
#include <utility>
#include <vector>

#include <boost/log/sinks/sync_frontend.hpp>
#include <boost/log/sinks/text_ostream_backend.hpp>
#include <boost/shared_ptr.hpp>

#include "libslic3r/BoundingBox.hpp"
#include "libslic3r/ExPolygon.hpp"
#include "libslic3r/ExtrusionEntity.hpp"
#include "libslic3r/ExtrusionEntityCollection.hpp"
#include "libslic3r/Fill/FillBase.hpp"
#include "libslic3r/Flow.hpp"
#include "libslic3r/Point.hpp"
#include "libslic3r/Polygon.hpp"
#include "libslic3r/Polyline.hpp"
#include "libslic3r/PrintConfig.hpp"
#include "libslic3r/Surface.hpp"

namespace probe {
namespace fill {

// ---------------------------------------------------------------- values (parity/sop1/fill.md)

void put(Payload &p, const Slic3r::Point &q);
void put_points(Payload &p, const Slic3r::Points &pts);
void put(Payload &p, const Slic3r::Polygon &q);
void put(Payload &p, const Slic3r::Polygons &qs);
void put(Payload &p, const Slic3r::ExPolygon &e);
void put(Payload &p, const Slic3r::ExPolygons &es);
void put(Payload &p, const Slic3r::Polyline &pl);
void put(Payload &p, const Slic3r::Polylines &pls);
void put(Payload &p, const Slic3r::ThickPolyline &t);
void put(Payload &p, const Slic3r::ThickPolylines &ts);
void put(Payload &p, const Slic3r::BoundingBox &b);
void put(Payload &p, const Slic3r::Surface &s);
void put(Payload &p, const Slic3r::Flow &f);
void put(Payload &p, const Slic3r::FillParams &q);
void put_entity(Payload &p, const Slic3r::ExtrusionEntity &e);
void put(Payload &p, const Slic3r::ExtrusionEntitiesPtr &es);

// The three configs a filler reads: the print's nozzle diameters, the object's gap fill target
// and the region's gap fill filter. Every other option keeps its default.
struct Configs
{
    Slic3r::PrintConfig       print;
    Slic3r::PrintObjectConfig object;
    Slic3r::PrintRegionConfig region;
};
void put(Payload &p, const Configs &c);

// A filler's state as the record's `filler` value: its fields and whether print_config and
// print_object_config point at the record's configs.
void put_filler(Payload &p, const Slic3r::Fill &f);

// Owns the entities of an ExtrusionEntitiesPtr and deletes them.
struct Entities
{
    Slic3r::ExtrusionEntitiesPtr v;
    Entities() = default;
    Entities(const Entities &) = delete;
    Entities &operator=(const Entities &) = delete;
    ~Entities();
};

// ---------------------------------------------------------------- generator

struct Gen : SplitMix
{
    explicit Gen(uint64_t seed) : SplitMix(seed) {}
    // A double in [lo, hi), from 53 random bits.
    double real(double lo, double hi) { return lo + (hi - lo) * (double(next() >> 11) * (1.0 / 9007199254740992.0)); }
    // An integer in [lo, hi].
    int64_t range(int64_t lo, int64_t hi) { return lo + int64_t(below(uint64_t(hi - lo) + 1)); }
    // True one time in n.
    bool one_in(uint64_t n) { return below(n) == 0; }
    template<class T> const T &pick(const std::vector<T> &v) { return v[size_t(below(v.size()))]; }
};

// ---------------------------------------------------------------- shapes

// Millimetres to scaled coordinates, as scale_ does.
int64_t mm(double v);
Slic3r::Polygon   rect(double x0, double y0, double w, double h);
Slic3r::Polygon   ngon(double cx, double cy, double r, int n, double phase);
Slic3r::Polygon   star(Gen &g, double cx, double cy, double r_in, double r_out, int n);
Slic3r::ExPolygon with_holes(Slic3r::Polygon contour, Slic3r::Polygons holes);
// The probe's own shapes, each named on the page: a square, a rectangle, a square with a square
// hole, an L, a comb, a thin strip, a disc, an annulus, a star, a dumbbell, a shape too small for
// one line, two test_fill.cpp shapes (OrcaSlicer's tests/fff_print/test_fill.cpp).
struct Shape
{
    std::string       name;
    Slic3r::ExPolygon e;
};
std::vector<Shape> hand_shapes();
// A hand shape at half its size (scaled about the origin), for the cases that reach a branch.
Slic3r::ExPolygon half(const Slic3r::ExPolygon &e);
// A seeded shape: one of the hand shapes moved, scaled and turned, or a star with holes.
Slic3r::ExPolygon random_shape(Gen &g, double size_mm);
// Sections of OrcaSlicer's test meshes (<exe>/../src/tests/data, the oracle's source tree), each
// ExPolygon of a slice at a few heights, largest first, as the page lists them.
bool mesh_sections(std::vector<Shape> &out, std::string &error);

// ---------------------------------------------------------------- the committed set

// Every case goes to the full set. The committed set takes the edge cases, then the first `cap`
// seeded cases of at most `max_bytes` each, in order, each while the kind's records there (with
// their 12 bytes of kind and length) stay within `budget` bytes; a case that does not fit is
// left out and the next ones are still tried.
class Recorder
{
public:
    Recorder(Sets &sets, const char (&kind)[5], size_t cap, size_t max_bytes, size_t budget = size_t(-1))
        : sets_(sets), kind_(kind), cap_(cap), max_bytes_(max_bytes), budget_(budget)
    {}
    void edge(const Payload &p);
    void seeded(const Payload &p);
    size_t committed() const { return committed_; }
    size_t full() const { return full_; }

private:
    bool        fits(const Payload &p) const { return budget_ == size_t(-1) || bytes_ + p.bytes.size() + 12 <= budget_; }
    void        commit(const Payload &p);
    Sets       &sets_;
    const char (&kind_)[5];
    size_t      cap_;
    size_t      max_bytes_;
    size_t      budget_;
    size_t      bytes_     = 0;
    size_t      taken_     = 0;
    size_t      committed_ = 0;
    size_t      full_      = 0;
};

// ---------------------------------------------------------------- what a call prints

// The engine's error log, as the command line prints it on stderr at its default level
// (Boost.Log records at error and fatal, "[<severity>] <message>"), held for the whole run so
// that Boost.Log's default sink stays off; take() returns what came since the last take().
class ErrorLog
{
public:
    ErrorLog();
    ErrorLog(const ErrorLog &) = delete;
    ErrorLog &operator=(const ErrorLog &) = delete;
    ~ErrorLog();
    std::string take();

private:
    std::ostringstream                                                                               text_;
    boost::shared_ptr<boost::log::sinks::synchronous_sink<boost::log::sinks::text_ostream_backend>> sink_;
};
// The run's ErrorLog, held by run_fill (fill.cpp) while it records.
class ErrorLogScope
{
public:
    ErrorLogScope();
    ErrorLogScope(const ErrorLogScope &) = delete;
    ErrorLogScope &operator=(const ErrorLogScope &) = delete;
    ~ErrorLogScope();

private:
    ErrorLog log_;
};
// What the engine logged at error or fatal since the last call ("" outside the scope).
std::string take_errors();

// Runs f with the C runtime's stdout sent to a scratch file in binary mode and returns what it
// wrote (printf's own bytes, with no line-end translation); false when stdout cannot be moved.
bool capture_stdout(const std::function<void()> &f, std::string &out, std::string &error);

// The outcome of a call that may throw: u8 0 and then its results, or u8 1 and str what().
void put_threw(Payload &p, const std::exception &e);

// ---------------------------------------------------------------- Fill's protected members

struct FillAccess : Slic3r::Fill
{
    static std::pair<float, Slic3r::Point> infill_direction(const Slic3r::Fill &f, const Slic3r::Surface *s)
    {
        return (f.*(&FillAccess::_infill_direction))(s);
    }
    static float layer_angle(const Slic3r::Fill &f, size_t idx) { return (f.*(&FillAccess::_layer_angle))(idx); }
    static void  create_gap_fill(Slic3r::Fill &f, const Slic3r::Surface *s, const Slic3r::FillParams &params, Slic3r::ExtrusionEntityCollection *out)
    {
        (f.*(&FillAccess::_create_gap_fill))(s, params, out);
    }
};

// The filler a record describes, set on a filler of any type.
struct FillerState
{
    size_t                layer_id   = size_t(-1);
    double                z          = 0.;
    double                spacing    = 0.;
    double                overlap    = 0.;
    float                 angle      = 0.f;
    bool                  fixed      = false;
    int64_t               link_max   = 0;
    int64_t               loop_clip  = 0;
    Slic3r::BoundingBox   bbox;
    bool                  dont_alt   = false;
    Slic3r::ExPolygons    no_overlap;
    bool                  configs    = true;
    void                  apply(Slic3r::Fill &f, const Configs &c) const;
};

// The records, by source file.
void record_base(Sets &sets, const std::vector<Shape> &sections);
bool record_direction(Sets &sets, std::string &error);
bool record_surface(Sets &sets, const std::vector<Shape> &sections, std::string &error);
void record_connect(Sets &sets, const std::vector<Shape> &sections);
void record_extrusion(Sets &sets, const std::vector<Shape> &sections);
void record_patterns(Sets &sets, const std::vector<Shape> &sections);
void record_types(Sets &sets);

// Lines of an infill over e: vertical lines `spacing_mm` apart turned by `angle`, shifted by
// `shift` (a fraction of the spacing), clipped by e (intersection_pl), as a pattern feeds
// connect_infill.
Slic3r::Polylines clipped_lines(const Slic3r::ExPolygon &e, double angle, double spacing_mm, double shift);

} // namespace fill
} // namespace probe
