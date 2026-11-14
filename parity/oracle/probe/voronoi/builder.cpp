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
// The voronoi probe's builder runs (parity/sop1/voronoi.md, VRQU, VRSS, VRBT and the predicate
// calls VRNC, VRDP, VRCE, VRCF, VRLZ, VRMP and VREC's captured part). Every site set runs through
// boost::polygon::voronoi_builder<int32, voronoi_ctype_traits<int32>, RecVP>: RecVP is
// voronoi_predicates<int32 traits> with its event comparison, node comparison and circle formation
// predicates replaced by classes that derive from the real ones, call them and keep a sample of
// their inputs and answers, so the samples are the calls the builder really makes. The output it
// builds into, TraceOut, forwards every call to a real voronoi_diagram<double> and keeps the call;
// the run fails when that diagram differs from the default builder's for the same input. Written by
// orca-o3-probe-voronoi, 2026-11-13.
#include "common.hpp"

#include <algorithm>
#include <utility>

namespace probe::vr {

namespace {

using Node = bpd::beach_line_node_key<Site>;
using ECmp = VP::event_comparison_predicate<Site, Circ>;
using PCmp = VP::point_comparison_predicate<Pt>;
using DP   = VP::distance_predicate<Site>;
using MP   = VP::mp_circle_formation_functor<Site, Circ>;
namespace bp = boost::polygon;

// ---------------------------------------------------------------- the capture

// Which calls of a run are kept: every call of a hand-made site set up to 40, else every 7th up to 8.
struct Sampler
{
    uint64_t seen = 0;
    size_t   kept = 0;
    void     reset()
    {
        seen = 0;
        kept = 0;
    }
    bool want(bool hand)
    {
        const uint64_t k      = seen++;
        const uint64_t stride = hand ? 1 : 7;
        const size_t   max    = hand ? 40 : 8;
        if (k % stride != 0 || kept >= max)
            return false;
        ++kept;
        return true;
    }
};

struct LzCall
{
    int  form;
    int  index;
    Site s1, s2, s3;
    Circ circle;
};

struct Capture
{
    bool                 on   = false;
    bool                 hand = false;
    bool                 formed = false;
    Sampler              ec, nc, ce, cf, lz;
    std::vector<Payload> vrec, vrnc, vrdp, vrce, vrcf, vrlz;
    std::vector<LzCall>  lz_calls;
    void                 start(bool h)
    {
        on   = true;
        hand = h;
        ec.reset();
        nc.reset();
        ce.reset();
        cf.reset();
        lz.reset();
    }
};

Capture g_cap;

void cap_ec(int form, const Site *s1, const Circ *c1, const Site *s2, const Circ *c2, bool res)
{
    if (!g_cap.on || !g_cap.ec.want(g_cap.hand))
        return;
    Payload p;
    p.u8(uint8_t(form));
    if (s1)
        put_site(p, *s1);
    else
        put_circle(p, *c1);
    if (s2)
        put_site(p, *s2);
    else
        put_circle(p, *c2);
    p.boolean(res);
    g_cap.vrec.push_back(std::move(p));
}

const Site &newer_site(const Node &n) { return n.left_site().sorted_index() > n.right_site().sorted_index() ? n.left_site() : n.right_site(); }
const Pt   &comparison_point(const Site &s) { return PCmp()(s.point0(), s.point1()) ? s.point0() : s.point1(); }

void cap_nc(const Node &a, const Node &b, bool res)
{
    if (!g_cap.on || !g_cap.nc.want(g_cap.hand))
        return;
    Payload p;
    put_site(p, a.left_site());
    put_site(p, a.right_site());
    put_site(p, b.left_site());
    put_site(p, b.right_site());
    p.boolean(res);
    g_cap.vrnc.push_back(std::move(p));
    // The distance predicate call this comparison makes (voronoi_predicates.hpp 389-404): with the
    // newer site's comparison point of each node, the node whose point lies left is measured
    // against the other's point; equal x makes no call.
    const Pt &p1 = comparison_point(newer_site(a));
    const Pt &p2 = comparison_point(newer_site(b));
    if (p1.x() == p2.x())
        return;
    const Node &n  = p1.x() < p2.x() ? a : b;
    const Pt   &pt = p1.x() < p2.x() ? p2 : p1;
    Payload     d;
    put_site(d, n.left_site());
    put_site(d, n.right_site());
    d.i32(pt.x()).i32(pt.y()).boolean(DP()(n.left_site(), n.right_site(), pt));
    g_cap.vrdp.push_back(std::move(d));
}

void cap_ce(int form, int index, const Site &s1, const Site &s2, const Site &s3, bool res)
{
    if (!g_cap.on || !g_cap.ce.want(g_cap.hand))
        return;
    Payload p;
    p.u8(uint8_t(form)).i32(index);
    put_site(p, s1);
    put_site(p, s2);
    put_site(p, s3);
    p.boolean(res);
    g_cap.vrce.push_back(std::move(p));
}

void cap_lz(int form, int index, const Site &s1, const Site &s2, const Site &s3, const Circ &c)
{
    g_cap.formed = true;
    if (!g_cap.on || !g_cap.lz.want(g_cap.hand))
        return;
    Payload p;
    p.u8(uint8_t(form)).i32(index);
    put_site(p, s1);
    put_site(p, s2);
    put_site(p, s3);
    put_circle(p, c);
    g_cap.vrlz.push_back(std::move(p));
    g_cap.lz_calls.push_back(LzCall{form, index, s1, s2, s3, c});
}

void cap_cf(const Site &s1, const Site &s2, const Site &s3, bool res, const Circ &c)
{
    if (!g_cap.on || !g_cap.cf.want(g_cap.hand))
        return;
    Payload p;
    put_site(p, s1);
    put_site(p, s2);
    put_site(p, s3);
    p.boolean(g_cap.formed).boolean(res);
    if (g_cap.formed)
        put_circle(p, c);
    g_cap.vrcf.push_back(std::move(p));
}

template<typename S> class RecCEP : public VP::circle_existence_predicate<S>
{
    using Base = VP::circle_existence_predicate<S>;

public:
    bool ppp(const S &a, const S &b, const S &c) const
    {
        const bool r = Base::ppp(a, b, c);
        cap_ce(0, 0, a, b, c, r);
        return r;
    }
    bool pps(const S &a, const S &b, const S &c, int segment_index) const
    {
        const bool r = Base::pps(a, b, c, segment_index);
        cap_ce(1, segment_index, a, b, c, r);
        return r;
    }
    bool pss(const S &a, const S &b, const S &c, int point_index) const
    {
        const bool r = Base::pss(a, b, c, point_index);
        cap_ce(2, point_index, a, b, c, r);
        return r;
    }
    bool sss(const S &a, const S &b, const S &c) const
    {
        const bool r = Base::sss(a, b, c);
        cap_ce(3, 0, a, b, c, r);
        return r;
    }
};

template<typename S, typename C> class RecLazy : public VP::lazy_circle_formation_functor<S, C>
{
    using Base = VP::lazy_circle_formation_functor<S, C>;

public:
    void ppp(const S &a, const S &b, const S &c, C &e)
    {
        Base::ppp(a, b, c, e);
        cap_lz(0, 0, a, b, c, e);
    }
    void pps(const S &a, const S &b, const S &c, int segment_index, C &e)
    {
        Base::pps(a, b, c, segment_index, e);
        cap_lz(1, segment_index, a, b, c, e);
    }
    void pss(const S &a, const S &b, const S &c, int point_index, C &e)
    {
        Base::pss(a, b, c, point_index, e);
        cap_lz(2, point_index, a, b, c, e);
    }
    void sss(const S &a, const S &b, const S &c, C &e)
    {
        Base::sss(a, b, c, e);
        cap_lz(3, 0, a, b, c, e);
    }
};

// voronoi_predicates with recording event comparison, node comparison and circle formation.
struct RecVP : VP
{
    template<typename S, typename C> class event_comparison_predicate : public VP::event_comparison_predicate<S, C>
    {
        using Base = VP::event_comparison_predicate<S, C>;

    public:
        bool operator()(const S &l, const S &r) const
        {
            const bool res = Base::operator()(l, r);
            cap_ec(0, &l, nullptr, &r, nullptr, res);
            return res;
        }
        bool operator()(const S &l, const C &r) const
        {
            const bool res = Base::operator()(l, r);
            cap_ec(1, &l, nullptr, nullptr, &r, res);
            return res;
        }
        bool operator()(const C &l, const S &r) const
        {
            const bool res = Base::operator()(l, r);
            cap_ec(2, nullptr, &l, &r, nullptr, res);
            return res;
        }
        bool operator()(const C &l, const C &r) const
        {
            const bool res = Base::operator()(l, r);
            cap_ec(3, nullptr, &l, nullptr, &r, res);
            return res;
        }
    };

    template<typename N> class node_comparison_predicate : public VP::node_comparison_predicate<N>
    {
        using Base = VP::node_comparison_predicate<N>;

    public:
        bool operator()(const N &a, const N &b) const
        {
            const bool res = Base::operator()(a, b);
            cap_nc(a, b, res);
            return res;
        }
    };

    template<typename S, typename C, typename CEP = RecCEP<S>, typename CFF = RecLazy<S, C>>
    class circle_formation_predicate : public VP::circle_formation_predicate<S, C, CEP, CFF>
    {
        using Base = VP::circle_formation_predicate<S, C, CEP, CFF>;

    public:
        bool operator()(const S &s1, const S &s2, const S &s3, C &c)
        {
            g_cap.formed = false;
            const bool res = Base::operator()(s1, s2, s3, c);
            cap_cf(s1, s2, s3, res, c);
            return res;
        }
    };
};

// ---------------------------------------------------------------- the trace

struct Call
{
    uint8_t  tag = 0;
    uint64_t n   = 0;
    Site     a, b;
    double   cx = 0, cy = 0, lx = 0;
    uint32_t d12 = 0, d23 = 0, r1 = 0, r2 = 0;
};

// The builder's output: a real voronoi_diagram<double> with every call kept.
struct TraceOut
{
    BVD               vd;
    std::vector<Call> calls;

    uint32_t idx(const void *e) const { return uint32_t(static_cast<const BVD::edge_type *>(e) - vd.edges().data()); }

    void _reserve(std::size_t n)
    {
        Call c;
        c.tag = 0;
        c.n   = n;
        calls.push_back(c);
        vd._reserve(n);
    }
    template<typename CT> void _process_single_site(const bpd::site_event<CT> &s)
    {
        Call c;
        c.tag = 1;
        c.a   = s;
        calls.push_back(c);
        vd._process_single_site(s);
    }
    template<typename CT> std::pair<void *, void *> _insert_new_edge(const bpd::site_event<CT> &s1, const bpd::site_event<CT> &s2)
    {
        std::pair<void *, void *> r = vd._insert_new_edge(s1, s2);
        Call                      c;
        c.tag = 2;
        c.a   = s1;
        c.b   = s2;
        c.r1  = idx(r.first);
        c.r2  = idx(r.second);
        calls.push_back(c);
        return r;
    }
    template<typename CT1, typename CT2>
    std::pair<void *, void *> _insert_new_edge(const bpd::site_event<CT1> &s1, const bpd::site_event<CT1> &s3, const bpd::circle_event<CT2> &circle, void *d12, void *d23)
    {
        Call c;
        c.tag = 3;
        c.a   = s1;
        c.b   = s3;
        c.cx  = circle.x();
        c.cy  = circle.y();
        c.lx  = circle.lower_x();
        c.d12 = idx(d12);
        c.d23 = idx(d23);
        std::pair<void *, void *> r = vd._insert_new_edge(s1, s3, circle, d12, d23);
        c.r1                        = idx(r.first);
        c.r2                        = idx(r.second);
        calls.push_back(c);
        return r;
    }
    void _build()
    {
        Call c;
        c.tag = 4;
        calls.push_back(c);
        vd._build();
    }
};

void put_ref(Payload &p, const Site &s) { p.u32(uint32_t(s.sorted_index())).boolean(s.is_inverse()); }

Payload trace_payload(const SiteSet &ss, const TraceOut &out)
{
    struct Entry
    {
        bool    seen = false;
        int32_t x0 = 0, y0 = 0, x1 = 0, y1 = 0;
        uint64_t initial  = 0;
        uint8_t  category = 0;
    };
    std::vector<Entry> table;
    auto               note = [&](const Site &s) {
        const size_t i = s.sorted_index();
        if (i >= table.size())
            table.resize(i + 1);
        Entry &e = table[i];
        if (e.seen)
            return;
        e.seen = true;
        // The site as sorted: an inverse site holds its points swapped.
        const Pt &a = s.is_inverse() ? s.point1() : s.point0();
        const Pt &b = s.is_inverse() ? s.point0() : s.point1();
        e.x0        = a.x();
        e.y0        = a.y();
        e.x1        = b.x();
        e.y1        = b.y();
        e.initial   = uint64_t(s.initial_index());
        e.category  = uint8_t(s.source_category());
    };
    for (const Call &c : out.calls)
        if (c.tag == 1)
            note(c.a);
        else if (c.tag == 2 || c.tag == 3) {
            note(c.a);
            note(c.b);
        }
    Payload p;
    put_siteset(p, ss);
    p.u32(uint32_t(table.size()));
    for (const Entry &e : table) {
        p.boolean(e.seen);
        if (e.seen)
            p.i32(e.x0).i32(e.y0).i32(e.x1).i32(e.y1).u64(e.initial).u8(e.category);
    }
    p.u32(uint32_t(out.calls.size()));
    for (const Call &c : out.calls) {
        p.u8(c.tag);
        switch (c.tag) {
        case 0: p.u64(c.n); break;
        case 1: put_ref(p, c.a); break;
        case 2:
            put_ref(p, c.a);
            put_ref(p, c.b);
            p.u32(c.r1).u32(c.r2);
            break;
        case 3:
            put_ref(p, c.a);
            put_ref(p, c.b);
            p.f64(c.cx).f64(c.cy).f64(c.lx).u32(c.d12).u32(c.d23).u32(c.r1).u32(c.r2);
            break;
        default: break;
        }
    }
    return p;
}

// The site array as insert_point and insert_segment make it (voronoi_builder.hpp 59-95).
std::vector<Site> sites_of(const SiteSet &ss)
{
    std::vector<Site> v;
    size_t            index = 0;
    for (const Pt32 &q : ss.points) {
        Site s(q.x, q.y);
        s.initial_index(index++).source_category(bp::SOURCE_CATEGORY_SINGLE_POINT);
        v.push_back(s);
    }
    for (const Seg32 &g : ss.segments) {
        const Pt p1(g.a.x, g.a.y), p2(g.b.x, g.b.y);
        Site     a(p1);
        a.initial_index(index).source_category(bp::SOURCE_CATEGORY_SEGMENT_START_POINT);
        v.push_back(a);
        Site b(p2);
        b.initial_index(index).source_category(bp::SOURCE_CATEGORY_SEGMENT_END_POINT);
        v.push_back(b);
        if (PCmp()(p1, p2)) {
            Site s(p1, p2);
            s.source_category(bp::SOURCE_CATEGORY_INITIAL_SEGMENT);
            s.initial_index(index);
            v.push_back(s);
        } else {
            Site s(p2, p1);
            s.source_category(bp::SOURCE_CATEGORY_REVERSE_SEGMENT);
            s.initial_index(index);
            v.push_back(s);
        }
        ++index;
    }
    return v;
}

// ---------------------------------------------------------------- VRQU

using QE = std::pair<Circ, uint32_t>;
// The builder's circle event order (voronoi_builder.hpp 160-165): the queue's top is the event the
// event comparison puts first.
struct QCmp
{
    bool operator()(const QE &l, const QE &r) const { return pred(r.first, l.first); }
    ECmp pred;
};

void record_queue(Sets &sets)
{
    Budget b{6, 2400};
    auto   run = [&](Gen &g, size_t ops, bool ties, bool hand) {
        bpd::ordered_queue<QE, QCmp> q;
        std::vector<QE *>            live;
        std::vector<bool>            in;
        size_t                       size = 0;
        Payload                      p;
        p.u32(uint32_t(ops));
        for (size_t i = 0; i < ops; ++i) {
            const uint64_t r = g.below(10);
            if (size == 0 || r < 5) {
                const Circ c = ties ? Circ(g.real(-1., 1.), double(g.below(2)), double(g.below(3))) : Circ(g.real(-9., 9.), g.real(-9., 9.), g.real(-9., 9.));
                const uint32_t id = uint32_t(live.size());
                QE            &e  = q.push(QE(c, id));
                live.push_back(&e);
                in.push_back(true);
                ++size;
                p.u8(0);
                put_circle(p, c);
                p.u32(id);
            } else if (r < 7) {
                std::vector<uint32_t> ids;
                for (uint32_t k = 0; k < uint32_t(in.size()); ++k)
                    if (in[k])
                        ids.push_back(k);
                const uint32_t id = ids[size_t(g.below(ids.size()))];
                live[id]->first.deactivate();
                p.u8(2).u32(id);
            } else {
                const QE &t = q.top();
                p.u8(1).u32(t.second).boolean(t.first.is_active());
                in[t.second] = false;
                q.pop();
                --size;
            }
        }
        put_record(sets, "VRQU", p, b.take(p, hand));
    };
    Gen g(0x0E0Eu);
    run(g, 16, true, true);
    run(g, 40, true, true);
    for (int i = 0; i < 40; ++i)
        run(g, size_t(g.range(20, 300)), g.chance(2), false);
}

// ---------------------------------------------------------------- the captured kinds

// Writes captured records: all to the full set, every stride-th (stride = count / cap) of the first
// to the committed set, at most `cap`.
void write_captured(Sets &sets, const char (&kind)[5], const std::vector<Payload> &recs, size_t cap)
{
    const size_t stride = std::max<size_t>(1, recs.size() / cap);
    size_t       taken  = 0;
    for (size_t i = 0; i < recs.size(); ++i) {
        const bool c = (i % stride) == 0 && taken < cap;
        if (c)
            ++taken;
        put_record(sets, kind, recs[i], c);
    }
}

std::vector<Payload> mp_records()
{
    std::vector<Payload> out;
    uint8_t              subset = 1;
    for (const LzCall &l : g_cap.lz_calls) {
        for (int pass = 0; pass < 2; ++pass) {
            const uint8_t flags = pass == 0 ? 7 : subset;
            if (pass == 1)
                subset = uint8_t(subset % 6 + 1);
            const bool fx = (flags & 1) != 0, fy = (flags & 2) != 0, fl = (flags & 4) != 0;
            Circ       c  = l.circle;
            MP         mp;
            switch (l.form) {
            case 0: mp.ppp(l.s1, l.s2, l.s3, c, fx, fy, fl); break;
            case 1: mp.pps(l.s1, l.s2, l.s3, l.index, c, fx, fy, fl); break;
            case 2: mp.pss(l.s1, l.s2, l.s3, l.index, c, fx, fy, fl); break;
            default: mp.sss(l.s1, l.s2, l.s3, c, fx, fy, fl); break;
            }
            Payload p;
            p.u8(uint8_t(l.form)).i32(l.index);
            put_site(p, l.s1);
            put_site(p, l.s2);
            put_site(p, l.s3);
            p.u8(flags);
            put_circle(p, l.circle);
            put_circle(p, c);
            out.push_back(std::move(p));
        }
    }
    return out;
}

} // namespace

bool record_builder(Sets &sets, std::string &error)
{
    record_queue(sets);
    Budget bss{2, 3000}, bbt{2, 3000};
    for (const SiteSet &ss : site_sets()) {
        {
            std::vector<Site> v = sites_of(ss);
            std::sort(v.begin(), v.end(), ECmp());
            v.erase(std::unique(v.begin(), v.end()), v.end());
            for (size_t i = 0; i < v.size(); ++i)
                v[i].sorted_index(i);
            Payload p;
            put_siteset(p, ss);
            p.u32(uint32_t(v.size()));
            for (const Site &s : v)
                put_site(p, s);
            put_record(sets, "VRSS", p, bss.take(p, ss.hand));
        }
        TraceOut out;
        g_cap.start(ss.hand);
        {
            bp::voronoi_builder<bpd::int32, CTT, RecVP> builder;
            for (const Pt32 &q : ss.points)
                builder.insert_point(q.x, q.y);
            for (const Seg32 &s : ss.segments)
                builder.insert_segment(s.a.x, s.a.y, s.b.x, s.b.y);
            builder.construct(&out);
        }
        g_cap.on = false;
        BVD ref;
        {
            bp::default_voronoi_builder builder;
            for (const Pt32 &q : ss.points)
                builder.insert_point(q.x, q.y);
            for (const Seg32 &s : ss.segments)
                builder.insert_segment(s.a.x, s.a.y, s.b.x, s.b.y);
            builder.construct(&ref);
        }
        Payload d1, d2;
        put_diagram(d1, out.vd);
        put_diagram(d2, ref);
        if (d1.bytes != d2.bytes) {
            error = "voronoi: the recording builder's diagram differs from the default builder's for the site set \"" + ss.label + "\"";
            return false;
        }
        const Payload p = trace_payload(ss, out);
        put_record(sets, "VRBT", p, bbt.take(p, ss.hand));
    }
    write_captured(sets, "VREC", g_cap.vrec, 60);
    write_captured(sets, "VRNC", g_cap.vrnc, 60);
    write_captured(sets, "VRDP", g_cap.vrdp, 60);
    write_captured(sets, "VRCE", g_cap.vrce, 80);
    write_captured(sets, "VRCF", g_cap.vrcf, 60);
    write_captured(sets, "VRLZ", g_cap.vrlz, 60);
    write_captured(sets, "VRMP", mp_records(), 60);
    return true;
}

} // namespace probe::vr
