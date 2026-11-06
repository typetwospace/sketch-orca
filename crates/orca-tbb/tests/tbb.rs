// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! orca-tbb against what a program built with MSVC 14.42 and linked with the engine's static
//! oneTBB 2021.5.0 recorded with one thread in an arena of concurrency P, for P from 1 to 64:
//! the leaves of `parallel_for` with each partitioner over `blocked_range<long long>`,
//! `blocked_range<size_t>`, `blocked_range<int>` (past `INT_MAX` included) and
//! `blocked_range2d<int>`; the strings a body builds through `parallel_reduce` and
//! `parallel_deterministic_reduce`, which show every leaf, split and join; the order of the
//! integer forms of `parallel_for` and of `parallel_for_each`; and the order in which
//! `parallel_invoke` and `task_group` run their functions, nested groups and a loop inside a task
//! included.
//!
//! The recording program printed one line per case. The tests here print the same lines from
//! orca-tbb in a serial arena and compare them: a few in full, and every line of each kind
//! through the FNV-1a digest of all of that kind's lines, with the line count. Besides, a
//! parallel arena must give the same leaves and the same reductions, run every index once,
//! and pass panics and cancellation on as oneTBB passes its exceptions.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use orca_tbb::*;

/// FNV-1a, as the recording program digests values: integers little-endian.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Fnv(14_695_981_039_346_656_037)
    }
    fn byte(&mut self, b: u8) {
        self.0 ^= u64::from(b);
        self.0 = self.0.wrapping_mul(1_099_511_628_211);
    }
    fn u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.byte(b);
        }
    }
    /// A string's bytes, then its length.
    fn str(&mut self, s: &str) {
        for &b in s.as_bytes() {
            self.byte(b);
        }
        self.u64(s.len() as u64);
    }
}

/// The lines of one kind, in the recording program's order.
struct Lines {
    lines: Vec<String>,
}

impl Lines {
    fn new() -> Self {
        Lines { lines: Vec::new() }
    }
    fn push(&mut self, s: String) {
        self.lines.push(s);
    }
    fn digest(&self) -> u64 {
        let mut f = Fnv::new();
        for l in &self.lines {
            for &b in l.as_bytes() {
                f.byte(b);
            }
            f.byte(b'\n');
        }
        f.0
    }
    fn check(&self, kind: &str, count: usize, digest: u64) {
        assert_eq!(self.lines.len(), count, "{kind}: number of lines");
        assert_eq!(self.digest(), digest, "{kind}: digest of the lines");
    }
    fn has(&self, line: &str) {
        assert!(self.lines.iter().any(|l| l == line), "missing line: {line}");
    }
}

/// The arenas' concurrencies.
const PS: [usize; 13] = [1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 24, 32, 64];
/// The lengths of the loops.
const NS: [i64; 32] = [
    0, 1, 2, 3, 4, 5, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65, 100, 127, 128, 129, 255, 256,
    257, 1000, 1023, 1024, 1025, 4096, 10007, 100_000,
];
/// The grain sizes.
const GS: [usize; 6] = [1, 2, 3, 5, 10, 100];

/// What each call of a loop's body records, in the order of the calls.
fn record<T: Send>(run: impl FnOnce(&(dyn Fn(T) + Sync))) -> Vec<T> {
    let out = Mutex::new(Vec::new());
    run(&|x| out.lock().unwrap().push(x));
    out.into_inner().unwrap()
}

fn partitioners() -> [(&'static str, Partitioner); 3] {
    [
        ("auto", Partitioner::Auto),
        ("simple", Partitioner::Simple),
        ("static", Partitioner::Static),
    ]
}

fn leaves_line(name: &str, part: Partitioner, p: usize, n: i64, g: usize) -> String {
    let leaves = record(|push| {
        parallel_for(BlockedRange::new(0_i64, n, g), part, |r| {
            push((r.begin(), r.end()))
        });
    });
    let mut f = Fnv::new();
    for &(b, e) in &leaves {
        f.u64(b as u64);
        f.u64(e as u64);
    }
    let mut s = format!(
        "for {name} {p} {n} {g} count={} digest={:016x}",
        leaves.len(),
        f.0
    );
    if leaves.len() <= 40 && n <= 100 {
        s.push_str(" |");
        for (b, e) in leaves {
            write!(s, " {b}-{e}").unwrap();
        }
    }
    s
}

fn size_t_line(name: &str, part: Partitioner, p: usize, b: usize, e: usize, g: usize) -> String {
    let leaves = record(|push| {
        parallel_for(BlockedRange::new(b, e, g), part, |r| {
            push((r.begin(), r.end()))
        });
    });
    let mut f = Fnv::new();
    for &(lb, le) in &leaves {
        f.u64(lb as u64);
        f.u64(le as u64);
    }
    format!(
        "for_size_t {name} {p} {b} {e} {g} count={} digest={:016x}",
        leaves.len(),
        f.0
    )
}

fn int_line(name: &str, part: Partitioner, p: usize, b: i32, e: i32, g: usize) -> String {
    let leaves = record(|push| {
        parallel_for(BlockedRange::new(b, e, g), part, |r| {
            push((r.begin(), r.end()))
        });
    });
    let mut f = Fnv::new();
    for &(lb, le) in &leaves {
        f.u64(i64::from(lb) as u64);
        f.u64(i64::from(le) as u64);
    }
    format!(
        "for_int {name} {p} {b} {e} {g} count={} digest={:016x}",
        leaves.len(),
        f.0
    )
}

fn line_2d(
    name: &str,
    part: Partitioner,
    p: usize,
    (r0, r1, rg): (i32, i32, usize),
    (c0, c1, cg): (i32, i32, usize),
) -> String {
    let leaves = record(|push| {
        parallel_for(BlockedRange2d::new(r0, r1, rg, c0, c1, cg), part, |r| {
            push([
                r.rows().begin(),
                r.rows().end(),
                r.cols().begin(),
                r.cols().end(),
            ]);
        });
    });
    let mut f = Fnv::new();
    for x in leaves.iter().flatten() {
        f.u64(i64::from(*x) as u64);
    }
    let mut s = format!(
        "for2d {name} {p} {r0} {r1} {rg} {c0} {c1} {cg} count={} digest={:016x}",
        leaves.len(),
        f.0
    );
    if leaves.len() <= 24 {
        s.push_str(" |");
        for [rb, re, cb, ce] in leaves {
            write!(s, " {rb}-{re}:{cb}-{ce}").unwrap();
        }
    }
    s
}

/// The loop lines of an arena of concurrency `p`, by kind.
fn loop_lines(
    p: usize,
    fors: &mut Lines,
    size_ts: &mut Lines,
    ints: &mut Lines,
    twods: &mut Lines,
) {
    let generated = Arena::serial(p).execute(|| {
        let (mut a, mut b, mut c, mut d) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for n in NS {
            for g in GS {
                for (name, part) in partitioners() {
                    a.push(leaves_line(name, part, p, n, g));
                }
            }
        }
        for g in [1, 7] {
            b.push(size_t_line("auto", Partitioner::Auto, p, 5, 1_000_005, g));
            b.push(size_t_line(
                "static",
                Partitioner::Static,
                p,
                3,
                4_000_000_003,
                g * 1000,
            ));
            c.push(int_line("auto", Partitioner::Auto, p, -1000, 777, g));
            c.push(int_line("static", Partitioner::Static, p, -7, i32::MAX, g));
            c.push(int_line("simple", Partitioner::Simple, p, -50, 50, g));
        }
        for rows in [1, 7, 40, 300] {
            for cols in [1, 9, 64, 500] {
                for g in [1, 16] {
                    d.push(line_2d(
                        "auto",
                        Partitioner::Auto,
                        p,
                        (-3, rows - 3, g),
                        (10, cols + 10, g),
                    ));
                    d.push(line_2d(
                        "simple",
                        Partitioner::Simple,
                        p,
                        (0, rows, g),
                        (0, cols, g * 2),
                    ));
                    d.push(line_2d(
                        "static",
                        Partitioner::Static,
                        p,
                        (0, rows, g),
                        (0, cols, g),
                    ));
                }
            }
        }
        (a, b, c, d)
    });
    for l in generated.0 {
        fors.push(l);
    }
    for l in generated.1 {
        size_ts.push(l);
    }
    for l in generated.2 {
        ints.push(l);
    }
    for l in generated.3 {
        twods.push(l);
    }
}

#[test]
fn loops_split_as_one_thread_in_an_arena_of_p() {
    let (mut fors, mut size_ts, mut ints, mut twods) =
        (Lines::new(), Lines::new(), Lines::new(), Lines::new());
    for p in PS {
        loop_lines(p, &mut fors, &mut size_ts, &mut ints, &mut twods);
    }
    fors.check("for", 7488, 0x236b_4d21_1491_aef1);
    size_ts.check("for_size_t", 52, 0x902c_f501_4352_6b71);
    ints.check("for_int", 78, 0xe3ca_976f_ae92_3a70);
    twods.check("for2d", 1248, 0x1d11_e20c_8758_1c18);
    fors.has("for static 3 100 1 count=3 digest=42bad02a49757dc1 | 0-33 33-67 67-100");
    fors.has(
        "for auto 4 100 3 count=36 digest=c0087f0655481081 | 0-3 3-6 6-9 9-12 12-15 15-18 18-21 \
         21-23 23-25 25-28 28-31 31-34 34-37 37-40 40-43 43-46 46-48 48-50 50-53 53-56 56-59 \
         59-62 62-65 65-68 68-71 71-73 73-75 75-78 78-81 81-84 84-87 87-90 90-93 93-96 96-98 \
         98-100",
    );
}

/// The recording program's reduction body: it writes each leaf as `[b,e)` and each join as
/// `(left+right)`.
#[derive(Default)]
struct Body {
    value: String,
}

impl ReduceBody<BlockedRange<i64>> for Body {
    fn split(&mut self) -> Self {
        Body::default()
    }
    fn apply(&mut self, r: BlockedRange<i64>) {
        write!(self.value, "[{},{})", r.begin(), r.end()).unwrap();
    }
    fn join(&mut self, right: Self) {
        self.value = format!("({}+{})", self.value, right.value);
    }
}

fn leaf_text(r: BlockedRange<i64>, init: String) -> String {
    format!("{init}[{},{})", r.begin(), r.end())
}

fn join_text(a: String, b: String) -> String {
    format!("({a}+{b})")
}

fn tree_line(what: &str, name: &str, p: usize, n: i64, g: usize, s: &str) -> String {
    let mut f = Fnv::new();
    f.str(s);
    let mut line = format!(
        "{what} {name} {p} {n} {g} len={} digest={:016x}",
        s.len(),
        f.0
    );
    if s.len() <= 600 {
        write!(line, " | {s}").unwrap();
    }
    line
}

/// The reduction lines of one case, in the recording program's order, with their kinds.
fn reduction_lines(p: usize, n: i64, g: usize) -> Vec<(&'static str, String)> {
    let range = BlockedRange::new(0_i64, n, g);
    let mut out = Vec::new();
    for (name, part) in partitioners() {
        let mut b = Body::default();
        parallel_reduce(range, part, &mut b);
        out.push(("reduce", tree_line("reduce", name, p, n, g, &b.value)));
        let s = parallel_reduce_fn(range, part, "I".to_string(), leaf_text, join_text);
        out.push(("reduce_fn", tree_line("reduce_fn", name, p, n, g, &s)));
    }
    for (name, part) in [
        ("simple", DeterministicPartitioner::Simple),
        ("static", DeterministicPartitioner::Static),
    ] {
        let mut b = Body::default();
        parallel_deterministic_reduce(range, part, &mut b);
        out.push((
            "det_reduce",
            tree_line("det_reduce", name, p, n, g, &b.value),
        ));
        let s =
            parallel_deterministic_reduce_fn(range, part, "I".to_string(), leaf_text, join_text);
        out.push((
            "det_reduce_fn",
            tree_line("det_reduce_fn", name, p, n, g, &s),
        ));
    }
    let mut b = Body::default();
    parallel_reduce(range, Partitioner::default(), &mut b);
    out.push(("reduce", tree_line("reduce", "default", p, n, g, &b.value)));
    let mut d = Body::default();
    parallel_deterministic_reduce(range, DeterministicPartitioner::default(), &mut d);
    out.push((
        "det_reduce",
        tree_line("det_reduce", "default", p, n, g, &d.value),
    ));
    out
}

const REDUCE_NS: [i64; 9] = [0, 1, 2, 5, 16, 17, 100, 1000, 4097];
const REDUCE_GS: [usize; 3] = [1, 3, 50];

#[test]
fn reductions_fold_and_join_as_one_thread_in_an_arena_of_p() {
    let (mut reduces, mut reduce_fns, mut dets, mut det_fns) =
        (Lines::new(), Lines::new(), Lines::new(), Lines::new());
    for p in PS {
        let lines = Arena::serial(p).execute(|| {
            let mut all = Vec::new();
            for n in REDUCE_NS {
                for g in REDUCE_GS {
                    all.extend(reduction_lines(p, n, g));
                }
            }
            all
        });
        for (kind, line) in lines {
            match kind {
                "reduce" => reduces.push(line),
                "reduce_fn" => reduce_fns.push(line),
                "det_reduce" => dets.push(line),
                _ => det_fns.push(line),
            }
        }
    }
    reduces.check("reduce", 1404, 0xddd4_2be5_a417_85cb);
    reduce_fns.check("reduce_fn", 1053, 0xbd54_ec13_d214_906c);
    dets.check("det_reduce", 1053, 0x1027_03f1_077b_2e10);
    det_fns.check("det_reduce_fn", 702, 0x1bf9_f3f3_7f34_a805);
    reduces.has(
        "reduce auto 4 17 3 len=47 digest=6ea1090350742d86 | \
         [0,2)[2,4)[4,6)[6,8)[8,10)[10,12)[12,14)[14,17)",
    );
    dets.has(
        "det_reduce simple 4 17 3 len=68 digest=e406834ca94e88f9 | \
         ((([0,2)+[2,4))+([4,6)+[6,8)))+(([8,10)+[10,12))+([12,14)+[14,17))))",
    );
    det_fns.has(
        "det_reduce_fn static 4 17 3 len=36 digest=99adde14cba2ea3d | \
         ((I[0,4)+I[4,8))+(I[8,12)+I[12,17)))",
    );
}

const INDEX_CASES: [(i32, i32, i32); 9] = [
    (0, 0, 1),
    (0, 1, 1),
    (0, 10, 1),
    (-5, 37, 1),
    (3, 100, 7),
    (0, 1000, 3),
    (10, 3, 1),
    (-20, 20, 40),
    (0, 100_000, 1),
];

fn digest_ints(v: &[i32]) -> u64 {
    let mut f = Fnv::new();
    for &i in v {
        f.u64(i64::from(i) as u64);
    }
    f.0
}

fn index_and_each_lines(p: usize) -> (Vec<String>, Vec<String>) {
    let mut index_lines = Vec::new();
    for (a, b, s) in INDEX_CASES {
        let order = record(|push| parallel_for_index_step(a, b, s, Partitioner::Auto, push));
        let order_s = record(|push| parallel_for_index_step(a, b, s, Partitioner::Static, push));
        let order_p = record(|push| parallel_for_index_step(a, b, s, Partitioner::Simple, push));
        let order_a = if s == 1 {
            record(|push| parallel_for_index(a, b, Partitioner::Auto, push))
        } else {
            Vec::new()
        };
        let mut line = format!(
            "for_index {p} {a} {b} {s} count={} digest={:016x} static={:016x} simple={:016x} \
             nostep={:016x}",
            order.len(),
            digest_ints(&order),
            digest_ints(&order_s),
            digest_ints(&order_p),
            digest_ints(&order_a)
        );
        if order.len() <= 40 {
            line.push_str(" |");
            for i in &order {
                write!(line, " {i}").unwrap();
            }
        }
        index_lines.push(line);
    }
    let mut each_lines = Vec::new();
    for n in [0, 1, 2, 5, 33, 100, 1000] {
        let mut items: Vec<i32> = (0..n).collect();
        let order = record(|push| parallel_for_each_mut(&mut items, |x| push(*x)));
        let order2 = record(|push| parallel_for_each(&items, |x| push(*x)));
        let mut line = format!(
            "for_each {p} {n} digest={:016x} range={:016x}",
            digest_ints(&order),
            digest_ints(&order2)
        );
        if n <= 33 {
            line.push_str(" |");
            for i in &order {
                write!(line, " {i}").unwrap();
            }
        }
        each_lines.push(line);
    }
    (index_lines, each_lines)
}

#[test]
fn integer_loops_and_for_each_run_in_order() {
    let (mut indexes, mut eaches) = (Lines::new(), Lines::new());
    for p in PS {
        let (i, e) = Arena::serial(p).execute(|| index_and_each_lines(p));
        for l in i {
            indexes.push(l);
        }
        for l in e {
            eaches.push(l);
        }
    }
    indexes.check("for_index", 117, 0xfcfa_d387_974d_5c2d);
    eaches.check("for_each", 91, 0x3e98_e0e1_7a2d_53e4);
    indexes.has(
        "for_index 4 3 100 7 count=14 digest=bd267c7ea3da21ec static=bd267c7ea3da21ec \
         simple=bd267c7ea3da21ec nostep=cbf29ce484222325 | 3 10 17 24 31 38 45 52 59 66 73 80 87 94",
    );
    eaches.has("for_each 4 5 digest=bde40bb18a01afc1 range=bde40bb18a01afc1 | 0 1 2 3 4");
}

fn order_text(order: &[i32]) -> String {
    let mut s = String::new();
    for i in order {
        write!(s, " {i}").unwrap();
    }
    s
}

/// Tasks that run more tasks in their group, `run_and_wait`, a second wave, and a group inside a
/// task, as the recording program runs them.
fn nested_group(push: &(dyn Fn(i32) + Sync)) {
    task_group(|tg| {
        tg.run(move |tg| {
            push(0);
            tg.run(move |_| push(10));
            tg.run(move |_| push(11));
            push(1);
        });
        tg.run(move |_| push(2));
        assert_eq!(
            tg.run_and_wait(move |tg| {
                push(3);
                tg.run(move |_| push(30));
            }),
            TaskGroupStatus::Complete
        );
        tg.run(move |_| push(4));
        tg.run(move |_| {
            push(5);
            task_group(|inner| {
                inner.run(move |_| push(50));
                inner.run(move |_| push(51));
                inner.wait();
                push(52);
            });
        });
        assert_eq!(tg.wait(), TaskGroupStatus::Complete);
    });
}

/// The invoke and task group lines of an arena of concurrency `p`, by kind.
fn task_lines(p: usize) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    out.push((
        "arena",
        format!("arena {p} max_concurrency={}", max_concurrency()),
    ));
    for n in 2..=10 {
        let order = record(|push| {
            parallel_invoke((0..n).map(|i| invoke_fn(move || push(i))).collect());
        });
        out.push(("invoke", format!("invoke {p} {n} |{}", order_text(&order))));
    }
    for n in 1..=6 {
        let order = record(|push| {
            task_group(|tg| {
                for i in 0..n {
                    tg.run(move |_| push(i));
                }
                push(-1);
                tg.wait();
            });
        });
        out.push((
            "task_group",
            format!("task_group {p} {n} |{}", order_text(&order)),
        ));
    }
    let order = record(nested_group);
    out.push((
        "task_group_nested",
        format!("task_group_nested {p} |{}", order_text(&order)),
    ));
    let order = record(|push| {
        task_group(|tg| {
            tg.run(move |_| push(100));
            tg.run(move |_| {
                parallel_for(BlockedRange::new(0, 8, 1), Partitioner::Simple, |r| {
                    push(r.begin())
                });
            });
            tg.run(move |_| push(200));
            tg.wait();
        });
    });
    out.push((
        "task_group_for",
        format!("task_group_for {p} |{}", order_text(&order)),
    ));
    out
}

#[test]
fn invoke_and_task_groups_run_in_onetbbs_order() {
    let mut by_kind: Vec<(&str, Lines)> = [
        "arena",
        "invoke",
        "task_group",
        "task_group_nested",
        "task_group_for",
    ]
    .into_iter()
    .map(|k| (k, Lines::new()))
    .collect();
    for p in PS {
        for (kind, line) in Arena::serial(p).execute(|| task_lines(p)) {
            let (_, lines) = by_kind.iter_mut().find(|(k, _)| *k == kind).unwrap();
            lines.push(line);
        }
    }
    let lines = |kind: &str| &by_kind.iter().find(|(k, _)| *k == kind).unwrap().1;
    lines("arena").check("arena", 13, 0x688a_c8ae_915e_3e4f);
    lines("invoke").check("invoke", 117, 0x7169_c26a_7488_7f82);
    lines("task_group").check("task_group", 78, 0x2067_0405_5844_1a9b);
    lines("task_group_nested").check("task_group_nested", 13, 0xfeff_f891_bdb5_efa4);
    lines("task_group_for").check("task_group_for", 13, 0x22a1_0663_39da_547e);
    lines("invoke").has("invoke 4 10 | 9 6 7 8 3 4 5 0 1 2");
    lines("task_group").has("task_group 4 4 | -1 3 2 1 0");
    lines("task_group_nested").has("task_group_nested 4 | 3 30 2 0 1 11 10 5 51 50 52 4");
    lines("task_group_for").has("task_group_for 4 | 200 0 1 2 3 4 5 6 7 100");
    lines("arena").has("arena 64 max_concurrency=64");
}

/// The concurrencies of the arenas with their own threads.
const PARALLEL_PS: [usize; 5] = [1, 2, 3, 4, 8];

#[test]
fn a_parallel_arena_runs_the_same_leaves_once_each() {
    for p in PARALLEL_PS {
        let arena = Arena::new(p).unwrap();
        for n in [0_i64, 1, 7, 100, 1025, 100_000] {
            for g in [1, 3, 100] {
                for (name, part) in partitioners() {
                    let range = BlockedRange::new(0_i64, n, g);
                    let expected: Vec<(i64, i64)> = leaves(range, part, p)
                        .iter()
                        .map(|r| (r.begin(), r.end()))
                        .collect();
                    let ran = arena.execute(|| {
                        record(|push| parallel_for(range, part, |r| push((r.begin(), r.end()))))
                    });
                    assert_eq!(ran.len(), expected.len(), "{name} {p} {n} {g}: leaves run");
                    let ran: Vec<_> = ran
                        .into_iter()
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect();
                    assert_eq!(ran, expected, "{name} {p} {n} {g}: leaves");
                }
            }
        }
    }
}

#[test]
fn a_parallel_arena_gives_the_same_reductions() {
    for p in PARALLEL_PS {
        let arena = Arena::new(p).unwrap();
        for n in REDUCE_NS {
            for g in REDUCE_GS {
                let serial = Arena::serial(p).execute(|| reduction_lines(p, n, g));
                let parallel = arena.execute(|| reduction_lines(p, n, g));
                assert_eq!(parallel, serial, "{p} {n} {g}");
            }
        }
    }
}

#[test]
fn a_parallel_arena_runs_every_index_and_item_once() {
    Arena::new(4).unwrap().execute(|| {
        for (a, b, s) in INDEX_CASES {
            for (name, part) in partitioners() {
                let ran = record(|push| parallel_for_index_step(a, b, s, part, push));
                let set: BTreeSet<i32> = ran.iter().copied().collect();
                assert_eq!(
                    set.len(),
                    ran.len(),
                    "{name} {a} {b} {s}: an index ran twice"
                );
                let expected: BTreeSet<i32> =
                    (0..).map(|k| a + k * s).take_while(|&v| v < b).collect();
                assert_eq!(set, expected, "{name} {a} {b} {s}");
            }
        }
        let mut items = vec![0_u32; 10_007];
        parallel_for_each_mut(&mut items, |x| *x += 1);
        assert!(items.iter().all(|&x| x == 1));
        let total = AtomicUsize::new(0);
        parallel_for_each(&items, |&x| {
            total.fetch_add(x as usize, Ordering::Relaxed);
        });
        assert_eq!(total.into_inner(), 10_007);
    });
}

#[test]
fn a_parallel_arena_runs_every_function_and_task() {
    for arena in [Arena::new(1).unwrap(), Arena::new(4).unwrap()] {
        arena.execute(|| {
            let ran = record(|push| {
                parallel_invoke((0..10).map(|i| invoke_fn(move || push(i))).collect());
            });
            assert_eq!(ran.len(), 10);
            assert_eq!(ran.into_iter().collect::<BTreeSet<_>>(), (0..10).collect());
            let ran = record(nested_group);
            assert_eq!(ran.len(), 12);
            let at = |v: i32| ran.iter().position(|&x| x == v).unwrap();
            assert!(at(0) < at(1) && at(3) < at(30) && at(5) < at(50));
            assert!(at(50) < at(52) && at(51) < at(52));
            let expected: BTreeSet<i32> = [0, 1, 2, 3, 4, 5, 10, 11, 30, 50, 51, 52].into();
            assert_eq!(ran.into_iter().collect::<BTreeSet<_>>(), expected);
        });
    }
    // Outside any arena: rayon's global pool, at the default concurrency.
    let ran = record(|push| parallel_invoke!(move || push(1), move || push(2), move || push(3)));
    assert_eq!(ran.into_iter().collect::<BTreeSet<_>>(), [1, 2, 3].into());
    assert_eq!(max_concurrency(), default_concurrency());
}

#[test]
fn the_leaves_see_their_arenas_concurrency() {
    let arena = Arena::new(3).unwrap();
    let seen = arena.execute(|| {
        record(|push| {
            parallel_for(
                BlockedRange::new(0, 64, 1),
                Partitioner::Simple,
                |_: BlockedRange<i32>| {
                    push(max_concurrency());
                },
            );
        })
    });
    assert_eq!(seen.len(), 64);
    assert!(seen.iter().all(|&c| c == 3));
}

fn panic_text(payload: Box<dyn std::any::Any + Send>) -> String {
    match payload.downcast::<&str>() {
        Ok(s) => (*s).to_string(),
        Err(payload) => payload.downcast::<String>().map(|s| *s).unwrap_or_default(),
    }
}

#[test]
fn a_panic_skips_the_leaves_not_started_and_comes_out_of_the_loop() {
    // One thread: leaves 0 to 3 run, and the panic of leaf 3 skips the rest.
    let ran = Mutex::new(Vec::new());
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
        Arena::serial(4).execute(|| {
            parallel_for(
                BlockedRange::new(0, 8, 1),
                Partitioner::Simple,
                |r: BlockedRange<i32>| {
                    ran.lock().unwrap().push(r.begin());
                    assert!(r.begin() != 3, "leaf 3");
                },
            );
        });
    }));
    assert_eq!(panic_text(outcome.unwrap_err()), "leaf 3");
    assert_eq!(ran.into_inner().unwrap(), [0, 1, 2, 3]);
    // A pool: the panic comes out, no leaf runs twice, and the arena goes on working.
    let arena = Arena::new(4).unwrap();
    let count = AtomicUsize::new(0);
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
        arena.execute(|| {
            parallel_for(
                BlockedRange::new(0, 1000, 1),
                Partitioner::Simple,
                |r: BlockedRange<i32>| {
                    count.fetch_add(1, Ordering::Relaxed);
                    assert!(r.begin() != 500, "leaf 500");
                },
            );
        });
    }));
    assert_eq!(panic_text(outcome.unwrap_err()), "leaf 500");
    assert!(count.into_inner() <= 1000);
    let ran = arena.execute(|| record(|push| parallel_for_index(0, 10, Partitioner::Auto, push)));
    assert_eq!(ran.len(), 10);
}

#[test]
fn a_cancelled_task_group_skips_what_has_not_started() {
    Arena::serial(2).execute(|| {
        // The newest task runs first and cancels the group; the older one is skipped. The
        // group takes tasks again after its wait.
        let ran = record(|push| {
            task_group(|tg| {
                tg.run(move |_| push(1));
                tg.run(move |tg| {
                    push(2);
                    tg.cancel();
                });
                assert_eq!(tg.wait(), TaskGroupStatus::Canceled);
                assert!(!tg.is_canceling());
                tg.run(move |_| push(3));
                assert_eq!(tg.wait(), TaskGroupStatus::Complete);
            });
        });
        assert_eq!(ran, [2, 3]);
        // A loop inside a task stops between leaves when the task's group is cancelled.
        let ran = record(|push| {
            task_group(|tg| {
                tg.run(move |tg| {
                    parallel_for(
                        BlockedRange::new(0, 8, 1),
                        Partitioner::Simple,
                        |r: BlockedRange<i32>| {
                            push(r.begin());
                            if r.begin() == 1 {
                                tg.cancel();
                            }
                        },
                    );
                });
                assert_eq!(tg.wait(), TaskGroupStatus::Canceled);
            });
        });
        assert_eq!(ran, [0, 1]);
    });
}

/// A body whose first leaf waits for an outer task group.
struct WaitingBody<'g, 's> {
    group: &'g TaskGroup<'s>,
}

impl ReduceBody<BlockedRange<i32>> for WaitingBody<'_, '_> {
    fn split(&mut self) -> Self {
        WaitingBody { group: self.group }
    }
    fn apply(&mut self, r: BlockedRange<i32>) {
        if r.begin() == 0 {
            self.group.wait();
        }
    }
    fn join(&mut self, _: Self) {}
}

#[test]
fn a_reduction_whose_leaf_lets_its_own_parts_start_beside_it_panics() {
    // oneTBB would split the body while its first leaf runs; a Rust body cannot be.
    let outcome = panic::catch_unwind(|| {
        Arena::serial(2).execute(|| {
            task_group(|tg| {
                tg.run(|_| ());
                parallel_reduce(
                    BlockedRange::new(0, 8, 1),
                    Partitioner::Simple,
                    &mut WaitingBody { group: tg },
                );
            });
        });
    });
    assert!(panic_text(outcome.unwrap_err()).contains("needed while it ran a leaf"));
}

#[test]
fn a_task_that_waits_for_its_own_group_panics() {
    let outcome = panic::catch_unwind(|| {
        Arena::serial(1).execute(|| {
            task_group(|tg| {
                tg.run(|tg| {
                    tg.wait();
                });
                tg.wait();
            });
        });
    });
    assert!(panic_text(outcome.unwrap_err()).contains("its own task group"));
}
