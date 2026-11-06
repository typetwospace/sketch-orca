// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-tbb: every record of `tbb.sop1` (`parity/SOP1.md`, "Probe files"),
//! which the probe program wrote with the engine's oneTBB as the parity oracle runs it (one
//! thread in an arena of concurrency P), replayed in [`Arena::serial`] arenas of the same P.
//!
//! - TBBA: `max_concurrency()` in each arena, and outside any arena.
//! - TBBF: the leaves of `parallel_for` over each range type with each partitioner, in the order
//!   they ran.
//! - TBBR: the trees `parallel_reduce` and `parallel_deterministic_reduce` build, in body and
//!   functional form.
//! - TBBO: the order of the integer `parallel_for`, `parallel_for_each`, `parallel_invoke` and
//!   `task_group` (nested, and with a loop inside a task).
//!
//! The committed subset (`parity/probes/`) runs by default; `SKETCH_ORCA_PROBES_DEEP=<folder>`
//! replays the full set from that folder (`<work>/parity/probes`).

use std::sync::Mutex;

use orca_probe::{Mismatches, ProbeFile, Reader, load_probe};
use orca_tbb::{
    Arena, BlockedRange, BlockedRange2d, DeterministicPartitioner, Partitioner, ReduceBody,
    invoke_fn, parallel_deterministic_reduce, parallel_deterministic_reduce_fn, parallel_for,
    parallel_for_each, parallel_for_index, parallel_for_index_step, parallel_invoke,
    parallel_reduce, parallel_reduce_fn, task_group,
};

fn file() -> ProbeFile {
    load_probe("tbb").unwrap_or_else(|e| panic!("{e}"))
}

fn arena(p: i32) -> Arena {
    Arena::serial(usize::try_from(p).expect("a positive concurrency"))
}

fn partitioner(name: &str) -> Partitioner {
    match name {
        "auto" | "default" => Partitioner::Auto,
        "simple" => Partitioner::Simple,
        "static" => Partitioner::Static,
        _ => panic!("a partitioner {name}"),
    }
}

fn deterministic(name: &str) -> DeterministicPartitioner {
    match name {
        "simple" | "default" => DeterministicPartitioner::Simple,
        "static" => DeterministicPartitioner::Static,
        _ => panic!("a deterministic partitioner {name}"),
    }
}

fn grain(g: i64) -> usize {
    usize::try_from(g).expect("a positive grain size")
}

/// A vector that the bodies of a loop push to.
struct Log<T>(Mutex<Vec<T>>);

impl<T> Log<T> {
    fn new() -> Self {
        Log(Mutex::new(Vec::new()))
    }

    fn push(&self, items: impl IntoIterator<Item = T>) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend(items);
    }

    fn take(self) -> Vec<T> {
        self.0
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[test]
fn max_concurrency_is_the_arenas() {
    let file = file();
    let mut m = Mismatches::default();
    for rec in file.of_kind("TBBA") {
        let mut r = rec.reader();
        let place = r.str().unwrap();
        let p = r.i32().unwrap();
        let want = r.i32().unwrap();
        r.finish("TBBA").unwrap();
        let got = match place.as_str() {
            "arena" => arena(p).execute(orca_tbb::max_concurrency),
            "outside" => {
                // Outside any arena oneTBB answers default_num_threads, the processor count of the
                // machine that recorded the probe: the same only on a machine with as many.
                let got = orca_tbb::max_concurrency();
                m.check(got == orca_tbb::default_concurrency(), || {
                    format!(
                        "max_concurrency() outside any arena is {got}, not default_concurrency()"
                    )
                });
                if orca_tbb::hardware_concurrency() != usize::try_from(want).unwrap() {
                    continue;
                }
                got
            }
            _ => panic!("a TBBA record {place}"),
        };
        m.check(got == usize::try_from(want).unwrap(), || {
            format!("max_concurrency() {place} P {p} is {got}, oneTBB {want}")
        });
    }
    m.assert_none("TBBA");
}

/// The leaves `parallel_for` runs, in order, for one TBBF record.
fn for_leaves(range: &str, part: Partitioner, p: i32, params: &[i64]) -> Vec<i64> {
    let log = Log::new();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap
    )]
    arena(p).execute(|| match range {
        "long long" => parallel_for(
            BlockedRange::new(params[0], params[1], grain(params[2])),
            part,
            |r| {
                log.push([r.begin(), r.end()]);
            },
        ),
        "size_t" => parallel_for(
            BlockedRange::new(params[0] as usize, params[1] as usize, grain(params[2])),
            part,
            |r: BlockedRange<usize>| log.push([r.begin() as i64, r.end() as i64]),
        ),
        "int" => parallel_for(
            BlockedRange::new(params[0] as i32, params[1] as i32, grain(params[2])),
            part,
            |r: BlockedRange<i32>| log.push([i64::from(r.begin()), i64::from(r.end())]),
        ),
        "2d" => parallel_for(
            BlockedRange2d::new(
                params[0] as i32,
                params[1] as i32,
                grain(params[2]),
                params[3] as i32,
                params[4] as i32,
                grain(params[5]),
            ),
            part,
            |r: BlockedRange2d<i32>| {
                let (rows, cols) = (r.rows(), r.cols());
                log.push([rows.begin(), rows.end(), cols.begin(), cols.end()].map(i64::from));
            },
        ),
        _ => panic!("a TBBF range {range}"),
    });
    log.take()
}

#[test]
fn parallel_for_runs_onetbbs_leaves_in_order() {
    let file = file();
    let mut m = Mismatches::default();
    for rec in file.of_kind("TBBF") {
        let mut r = rec.reader();
        let range = r.str().unwrap();
        let part = r.str().unwrap();
        let p = r.i32().unwrap();
        let params = r.vec(Reader::i64).unwrap();
        let want = r.vec(Reader::i64).unwrap();
        r.finish("TBBF").unwrap();
        let got = for_leaves(&range, partitioner(&part), p, &params);
        m.check(got == want, || {
            format!(
                "parallel_for {range} {part} P {p} {params:?}: {} leaves (oneTBB {}), first differing value at {:?}",
                got.len() / 2,
                want.len() / 2,
                got.iter().zip(&want).position(|(a, b)| a != b)
            )
        });
    }
    m.assert_none("TBBF");
}

/// The probe's reduction body: a leaf appends `[b,e)`, a join makes `(left+right)`.
#[derive(Default)]
struct Tree(String);

impl ReduceBody<BlockedRange<i64>> for Tree {
    fn split(&mut self) -> Self {
        Tree::default()
    }

    fn apply(&mut self, r: BlockedRange<i64>) {
        self.0 += &format!("[{},{})", r.begin(), r.end());
    }

    fn join(&mut self, right: Self) {
        self.0 = format!("({}+{})", self.0, right.0);
    }
}

fn leaf(r: BlockedRange<i64>, init: String) -> String {
    format!("{init}[{},{})", r.begin(), r.end())
}

fn join(a: String, b: String) -> String {
    format!("({a}+{b})")
}

fn reduce_tree(algo: &str, part: &str, p: i32, n: i64, g: usize) -> String {
    let range = BlockedRange::new(0i64, n, g);
    arena(p).execute(|| match algo {
        "reduce" => {
            let mut t = Tree::default();
            parallel_reduce(range, partitioner(part), &mut t);
            t.0
        }
        "reduce_fn" => parallel_reduce_fn(range, partitioner(part), "I".to_string(), leaf, join),
        "det_reduce" => {
            let mut t = Tree::default();
            parallel_deterministic_reduce(range, deterministic(part), &mut t);
            t.0
        }
        "det_reduce_fn" => parallel_deterministic_reduce_fn(
            range,
            deterministic(part),
            "I".to_string(),
            leaf,
            join,
        ),
        _ => panic!("a TBBR algorithm {algo}"),
    })
}

#[test]
fn reductions_build_onetbbs_trees() {
    let file = file();
    let mut m = Mismatches::default();
    for rec in file.of_kind("TBBR") {
        let mut r = rec.reader();
        let algo = r.str().unwrap();
        let part = r.str().unwrap();
        let p = r.i32().unwrap();
        let n = r.i64().unwrap();
        let g = usize::try_from(r.u64().unwrap()).unwrap();
        let want = r.str().unwrap();
        r.finish("TBBR").unwrap();
        let got = reduce_tree(&algo, &part, p, n, g);
        m.check(got == want, || {
            format!("{algo} {part} P {p} n {n} grain {g}: {got:?}, oneTBB {want:?}")
        });
    }
    m.assert_none("TBBR");
}

fn order(what: &str, variant: &str, p: i32, params: &[i64]) -> Vec<i32> {
    let log = Log::new();
    let push = |i: i32| log.push([i]);
    let count = || i32::try_from(params[0]).unwrap();
    arena(p).execute(|| match what {
        "for_index" => {
            let [a, b, s] = [0, 1, 2].map(|i| i32::try_from(params[i]).unwrap());
            match variant {
                "nostep" => parallel_for_index(a, b, Partitioner::Auto, push),
                _ => parallel_for_index_step(a, b, s, partitioner(variant), push),
            }
        }
        "for_each" => {
            // Both forms are parallel_for_each over the vector's random-access iterators.
            let items: Vec<i32> = (0..count()).collect();
            parallel_for_each(&items, |x| push(*x));
        }
        "invoke" => parallel_invoke((0..count()).map(|i| invoke_fn(move || push(i))).collect()),
        "task_group" => task_group(|tg| {
            for i in 0..count() {
                tg.run(move |_| push(i));
            }
            push(-1);
            tg.wait();
        }),
        "task_group_nested" => task_group(|tg| {
            tg.run(move |tg| {
                push(0);
                tg.run(move |_| push(10));
                tg.run(move |_| push(11));
                push(1);
            });
            tg.run(move |_| push(2));
            tg.run_and_wait(|tg| {
                push(3);
                tg.run(move |_| push(30));
            });
            tg.run(move |_| push(4));
            tg.run(move |_| {
                push(5);
                task_group(|inner| {
                    inner.run(move |_| push(50));
                    inner.run(move |_| push(51));
                    inner.wait();
                });
                push(52);
            });
            tg.wait();
        }),
        "task_group_for" => task_group(|tg| {
            tg.run(move |_| push(100));
            tg.run(move |_| {
                parallel_for(BlockedRange::new(0i32, 8, 1), Partitioner::Simple, |r| {
                    push(r.begin())
                });
            });
            tg.run(move |_| push(200));
            tg.wait();
        }),
        _ => panic!("a TBBO record for {what}"),
    });
    log.take()
}

#[test]
fn loops_invoke_and_task_groups_run_in_onetbbs_order() {
    let file = file();
    let mut m = Mismatches::default();
    for rec in file.of_kind("TBBO") {
        let mut r = rec.reader();
        let what = r.str().unwrap();
        let variant = r.str().unwrap();
        let p = r.i32().unwrap();
        let params = r.vec(Reader::i64).unwrap();
        let want = r.vec(Reader::i32).unwrap();
        r.finish("TBBO").unwrap();
        let got = order(&what, &variant, p, &params);
        m.check(got == want, || {
            format!("{what} {variant} P {p} {params:?}: {got:?}, oneTBB {want:?}")
        });
    }
    m.assert_none("TBBO");
}
