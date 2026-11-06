// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! orca-stl against what a program built with MSVC 14.42 (`cl /O2 /MT /EHsc /std:c++17`) recorded
//! for the same inputs: every comparator call of `std::sort`, `std::stable_sort` (with every
//! temporary-buffer size the engine can meet), `std::nth_element`, `std::partial_sort`,
//! `std::inplace_merge`, the heap functions, `std::lower_bound`, `std::upper_bound` and
//! `std::unique`, as FNV-1a digests of the call log and of the result; `std::min`, `std::max`,
//! `std::clamp` and the element searches on NaNs and signed zeros; `std::hash`; the iteration
//! order and bucket counts of `std::unordered_map` and `std::unordered_set` through inserts,
//! erases, copies, assignment, `reserve`, `rehash` and `clear`; and `std::mt19937`,
//! `std::mt19937_64`, `std::seed_seq`, `std::uniform_int_distribution`,
//! `std::uniform_real_distribution`, `std::generate_canonical` and `std::shuffle`.
//!
//! The recording program, `parity/oracle/probe/recorded/msvc_stl.cpp`, printed one line per case.
//! The tests here print the same lines from orca-stl and compare them: a few in full, and every line of each kind through the FNV-1a
//! digest of all of that kind's lines, with the line count. Besides, properties every result
//! must have (sorted, a permutation, stable, a heap) are checked on the same inputs.

use orca_stl::minmax::{max_of, max_of_f32, max_of_f64, min_of, min_of_f64};
use orca_stl::unordered::hash_bytes;
use orca_stl::*;

/// FNV-1a, as the recording program digests values: integers little-endian.
#[derive(Clone, Copy)]
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Fnv(14_695_981_039_346_656_037)
    }
    fn byte(&mut self, b: u8) {
        self.0 ^= u64::from(b);
        self.0 = self.0.wrapping_mul(1_099_511_628_211);
    }
    fn u32(&mut self, v: u32) {
        for b in v.to_le_bytes() {
            self.byte(b);
        }
    }
    fn u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.byte(b);
        }
    }
}

/// The recording program's input generator (splitmix64).
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// An element: a key to compare and the index it started at.
#[derive(Clone, Copy, Debug, PartialEq)]
struct E {
    key: i32,
    id: i32,
}

/// The comparator log: every call's two ids, digested, and the number of calls.
struct Log {
    f: Fnv,
    calls: u64,
}

impl Log {
    fn new() -> Self {
        Log {
            f: Fnv::new(),
            calls: 0,
        }
    }
    fn add(&mut self, a: i32, b: i32) {
        self.f.u32(a as u32);
        self.f.u32(b as u32);
        self.calls += 1;
    }
}

fn ids_of(v: &[E]) -> u64 {
    let mut f = Fnv::new();
    for e in v {
        f.u32(e.id as u32);
    }
    f.0
}

/// Patterns: 0 random in [0, range), 1 ascending, 2 descending, 3 all equal, 4 organ pipe,
/// 5 sawtooth with period `range`.
fn make(n: i32, pattern: i32, range: u64, seed: u64) -> Vec<E> {
    let mut sm = SplitMix(seed);
    (0..n)
        .map(|i| {
            let key = match pattern {
                0 => (sm.next() % range) as i32,
                1 => i,
                2 => n - i,
                3 => 7,
                4 => {
                    if i < n / 2 {
                        i
                    } else {
                        n - i
                    }
                }
                _ => (i as u64 % range) as i32,
            };
            E { key, id: i }
        })
        .collect()
}

const SIZES: [i32; 39] = [
    0, 1, 2, 3, 4, 5, 8, 15, 16, 31, 32, 33, 34, 40, 41, 42, 47, 63, 64, 65, 100, 127, 128, 129,
    200, 255, 256, 300, 511, 512, 513, 1000, 1023, 1024, 1025, 2000, 4096, 5000, 10007,
];
const RANGES: [u64; 4] = [2, 10, 1000, 2_147_483_647];

/// The cases of the algorithm lines: (n, pattern, range, seed).
fn cases() -> Vec<(i32, i32, u64, u64)> {
    let mut out = Vec::new();
    for n in SIZES {
        for pat in 0..6 {
            for range in RANGES {
                if pat != 0 && pat != 5 && range != 2 {
                    continue;
                }
                let seed = n as u64 * 1_000_003 + pat as u64 * 101 + range;
                out.push((n, pat, range, seed));
            }
        }
    }
    out
}

fn by_key_then_id(a: &E, b: &E) -> bool {
    a.key < b.key || (a.key == b.key && a.id < b.id)
}

/// The lines of one kind and their FNV-1a digest (each line's bytes and a newline).
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

fn sort_lines() -> (Lines, Lines, Lines) {
    let (mut sorts, mut stables, mut caps) = (Lines::new(), Lines::new(), Lines::new());
    for (n, pat, range, seed) in cases() {
        let mut v = make(n, pat, range, seed);
        let mut log = Log::new();
        sort_by(&mut v, |a, b| {
            log.add(a.id, b.id);
            a.key < b.key
        });
        assert!(
            v.windows(2).all(|w| w[0].key <= w[1].key),
            "sort {n} {pat} {range}: not sorted"
        );
        sorts.push(format!(
            "sort {n} {pat} {range} calls={} log={:016x} out={:016x}",
            log.calls,
            log.f.0,
            ids_of(&v)
        ));

        let mut v = make(n, pat, range, seed);
        let mut log = Log::new();
        stable_sort_by(&mut v, |a, b| {
            log.add(a.id, b.id);
            a.key < b.key
        });
        assert!(
            v.windows(2).all(|w| by_key_then_id(&w[0], &w[1])),
            "stable_sort {n} {pat} {range}: not stable"
        );
        stables.push(format!(
            "stable_sort {n} {pat} {range} calls={} log={:016x} out={:016x}",
            log.calls,
            log.f.0,
            ids_of(&v)
        ));

        if n > 32 {
            for cap in [1usize, 2, 3, 5, 16, 31, 32, 33, 100] {
                let mut v = make(n, pat, range, seed);
                let mut log = Log::new();
                stable_sort_by_with_capacity(&mut v, cap, |a, b| {
                    log.add(a.id, b.id);
                    a.key < b.key
                });
                assert!(v.windows(2).all(|w| by_key_then_id(&w[0], &w[1])));
                caps.push(format!(
                    "stable_sort_cap {n} {pat} {range} {cap} calls={} log={:016x} out={:016x}",
                    log.calls,
                    log.f.0,
                    ids_of(&v)
                ));
            }
        }
    }
    (sorts, stables, caps)
}

fn selection_lines() -> (Lines, Lines, Lines) {
    let (mut nths, mut partials, mut merges) = (Lines::new(), Lines::new(), Lines::new());
    for (n, pat, range, seed) in cases() {
        for nth in [0, 1, n / 3, n / 2, n - 1, n] {
            if nth < 0 || nth > n {
                continue;
            }
            let mut v = make(n, pat, range, seed);
            let mut log = Log::new();
            nth_element_by(&mut v, nth as usize, |a, b| {
                log.add(a.id, b.id);
                a.key < b.key
            });
            if nth < n {
                let k = v[nth as usize].key;
                assert!(v[..nth as usize].iter().all(|e| e.key <= k));
                assert!(v[nth as usize..].iter().all(|e| e.key >= k));
            }
            nths.push(format!(
                "nth_element {n} {pat} {range} {nth} calls={} log={:016x} out={:016x}",
                log.calls,
                log.f.0,
                ids_of(&v)
            ));
        }
        for mid in [0, 1, n / 4, n / 2, n] {
            if mid < 0 || mid > n {
                continue;
            }
            let mut v = make(n, pat, range, seed);
            let mut log = Log::new();
            partial_sort_by(&mut v, mid as usize, |a, b| {
                log.add(a.id, b.id);
                a.key < b.key
            });
            let mid = mid as usize;
            assert!(v[..mid].windows(2).all(|w| w[0].key <= w[1].key));
            if mid > 0 {
                assert!(v[mid..].iter().all(|e| e.key >= v[mid - 1].key));
            }
            partials.push(format!(
                "partial_sort {n} {pat} {range} {mid} calls={} log={:016x} out={:016x}",
                log.calls,
                log.f.0,
                ids_of(&v)
            ));
        }
        {
            let mut v = make(n, pat, range, seed);
            let mid = (n / 3) as usize;
            sort_by(&mut v[..mid], by_key_then_id);
            sort_by(&mut v[mid..], by_key_then_id);
            let mut log = Log::new();
            inplace_merge_by(&mut v, mid, |a, b| {
                log.add(a.id, b.id);
                a.key < b.key
            });
            assert!(v.windows(2).all(|w| w[0].key <= w[1].key));
            merges.push(format!(
                "inplace_merge {n} {pat} {range} calls={} log={:016x} out={:016x}",
                log.calls,
                log.f.0,
                ids_of(&v)
            ));
        }
    }
    (nths, partials, merges)
}

fn heap_and_search_lines() -> (Lines, Lines, Lines) {
    let (mut heaps, mut bounds, mut uniques) = (Lines::new(), Lines::new(), Lines::new());
    for (n, pat, range, seed) in cases() {
        {
            let mut v = make(n, pat, range, seed);
            let mut log = Log::new();
            let u = v.clone();
            let until = is_heap_until_by(&u, |a, b| {
                log.add(a.id, b.id);
                a.key < b.key
            });
            make_heap_by(&mut v, |a, b| {
                log.add(a.id, b.id);
                a.key < b.key
            });
            assert!(is_heap_by(&v, |a, b| a.key < b.key));
            let made = ids_of(&v);
            for k in (2..=n as usize).rev() {
                pop_heap_by(&mut v[..k], |a, b| {
                    log.add(a.id, b.id);
                    a.key < b.key
                });
            }
            let popped = ids_of(&v);
            for k in 1..=n as usize {
                push_heap_by(&mut v[..k], |a, b| {
                    log.add(a.id, b.id);
                    a.key < b.key
                });
            }
            sort_heap_by(&mut v, |a, b| {
                log.add(a.id, b.id);
                a.key < b.key
            });
            assert!(v.windows(2).all(|w| w[0].key <= w[1].key));
            heaps.push(format!(
                "heap {n} {pat} {range} until={until} calls={} log={:016x} made={made:016x} popped={popped:016x} out={:016x}",
                log.calls,
                log.f.0,
                ids_of(&v)
            ));
        }
        {
            let mut v = make(n, pat, range, seed);
            sort_by(&mut v, by_key_then_id);
            let mut log = Log::new();
            let mut res = Fnv::new();
            let mut sm = SplitMix(seed ^ 0x5555);
            for q in 0..40 {
                let key = if n == 0 {
                    0
                } else if q < 3 {
                    match q {
                        0 => -1,
                        1 => v[0].key,
                        _ => v[v.len() - 1].key + 1,
                    }
                } else {
                    // `v[splitmix() % n].key + int(splitmix() % 3) - 1`: MSVC called the
                    // right-hand splitmix() first.
                    let offset = (sm.next() % 3) as i32;
                    let at = (sm.next() % n as u64) as usize;
                    v[at].key + offset - 1
                };
                let probe = E { key, id: -1 };
                let lb = lower_bound_by(&v, &probe, |a: &E, b: &E| {
                    log.add(a.id, b.id);
                    a.key < b.key
                });
                let ub = upper_bound_by(&v, &probe, |a: &E, b: &E| {
                    log.add(a.id, b.id);
                    a.key < b.key
                });
                assert!(lb <= ub);
                assert!(v[..lb].iter().all(|e| e.key < key) && v[ub..].iter().all(|e| e.key > key));
                res.u64(lb as u64);
                res.u64(ub as u64);
            }
            bounds.push(format!(
                "bounds {n} {pat} {range} calls={} log={:016x} out={:016x}",
                log.calls, log.f.0, res.0
            ));
        }
        {
            let mut v = make(n, pat, range, seed);
            sort_by(&mut v, by_key_then_id);
            let mut log = Log::new();
            unique_vec_by(&mut v, |a, b| {
                log.add(a.id, b.id);
                a.key == b.key
            });
            assert!(v.windows(2).all(|w| w[0].key < w[1].key));
            uniques.push(format!(
                "unique {n} {pat} {range} len={} calls={} log={:016x} out={:016x}",
                v.len(),
                log.calls,
                log.f.0,
                ids_of(&v)
            ));
        }
    }
    (heaps, bounds, uniques)
}

#[test]
fn sort_and_stable_sort_make_msvcs_comparisons() {
    let (sorts, stables, caps) = sort_lines();
    sorts.has("sort 100 0 10 calls=677 log=4cbefc23b91e1571 out=527bcaed8eddc9c5");
    sorts.has("sort 100 4 2 calls=815 log=0510e37297cff751 out=4d05b0849b89ba55");
    stables.has("stable_sort 100 0 10 calls=925 log=8caafe98f2ad0a43 out=5548bb9b2d6685b5");
    caps.has("stable_sort_cap 100 0 10 1 calls=1006 log=8b21c092ddec24dc out=5548bb9b2d6685b5");
    caps.has("stable_sort_cap 100 0 10 31 calls=894 log=7fb8fcd97347cb1b out=5548bb9b2d6685b5");
    sorts.check("sort", SORT_LINES.0, SORT_LINES.1);
    stables.check("stable_sort", STABLE_SORT_LINES.0, STABLE_SORT_LINES.1);
    caps.check(
        "stable_sort_cap",
        STABLE_SORT_CAP_LINES.0,
        STABLE_SORT_CAP_LINES.1,
    );
}

#[test]
fn selections_and_merges_make_msvcs_comparisons() {
    let (nths, partials, merges) = selection_lines();
    nths.has("nth_element 100 0 10 33 calls=499 log=bcd20e18ce42c928 out=97d543ebfe319915");
    partials.has("partial_sort 100 0 10 25 calls=344 log=a188a3eac79af807 out=191db212e0716115");
    merges.has("inplace_merge 100 0 10 calls=96 log=6db082d5ae2c66be out=5548bb9b2d6685b5");
    nths.check("nth_element", NTH_ELEMENT_LINES.0, NTH_ELEMENT_LINES.1);
    partials.check("partial_sort", PARTIAL_SORT_LINES.0, PARTIAL_SORT_LINES.1);
    merges.check(
        "inplace_merge",
        INPLACE_MERGE_LINES.0,
        INPLACE_MERGE_LINES.1,
    );
}

#[test]
fn heaps_bounds_and_unique_make_msvcs_comparisons() {
    let (heaps, bounds, uniques) = heap_and_search_lines();
    heaps.has(
        "heap 100 0 10 until=6 calls=1508 log=04c0b4c69d3ff2fd made=d853639f34498ef5 popped=f303d93ae78c4075 out=c1afe1ec38732e95",
    );
    bounds.has("bounds 100 0 10 calls=524 log=070067d0416191f4 out=84178f1c2ab7a702");
    uniques.has("unique 100 0 10 len=10 calls=99 log=677e0d7d8c95e32e out=b83f334e20980a6a");
    heaps.check("heap", HEAP_LINES.0, HEAP_LINES.1);
    bounds.check("bounds", BOUNDS_LINES.0, BOUNDS_LINES.1);
    uniques.check("unique", UNIQUE_LINES.0, UNIQUE_LINES.1);
}

#[test]
fn small_sorts_make_the_recorded_calls() {
    // The whole call log, pair by pair, of std::sort on 5 and 12 keys.
    let expect = [
        (
            5,
            "keys 6 9 0 2 4 | out 2 3 4 0 1 | calls 11 | 1 0 1 0 2 0 3 2 3 1 3 0 3 2 4 2 4 1 4 0 4 3",
        ),
        (
            12,
            "keys 7 9 4 6 2 2 3 9 5 9 3 0 | out 11 4 5 6 10 2 8 3 0 1 7 9 | calls 40 | 1 0 1 0 2 0 3 2 3 1 3 0 3 2 4 2 5 4 5 1 5 0 5 3 5 2 5 4 6 4 6 1 6 0 6 3 6 2 6 5 7 4 7 1 8 4 8 7 8 1 8 0 8 3 8 2 9 4 9 7 10 4 10 9 10 7 10 1 10 0 10 3 10 8 10 2 10 6 11 4",
        ),
        (
            33,
            "keys 9 5 8 1 4 8 3 1 4 2 9 7 2 3 9 2 0 9 9 4 1 1 1 6 3 6 5 0 1 0 3 1 6 | out 16 27 29 3 22 7 21 20 28 31 9 12 15 24 6 13 30 4 8 19 1 26 32 23 25 11 5 2 18 14 10 17 0 | calls 226",
        ),
        (
            45,
            "keys 2 1 4 3 2 0 1 9 6 5 3 6 7 8 6 0 1 1 7 0 6 9 5 3 7 1 7 0 9 2 5 4 5 6 4 3 6 1 9 1 0 3 8 4 8 | out 5 40 15 27 19 1 39 25 17 6 16 37 29 0 4 3 10 23 35 41 2 31 34 43 9 22 30 32 8 11 33 20 36 14 12 24 26 18 13 42 44 7 21 28 38 | calls 360",
        ),
    ];
    for (n, line) in expect {
        let keys = make(n, 0, 10, 99 + n as u64);
        let mut v = keys.clone();
        let mut calls = Vec::new();
        sort_by(&mut v, |a, b| {
            calls.push(a.id);
            calls.push(b.id);
            a.key < b.key
        });
        let join = |it: &mut dyn Iterator<Item = i32>| {
            it.map(|x| x.to_string()).collect::<Vec<_>>().join(" ")
        };
        let mut got = format!(
            "keys {} | out {} | calls {}",
            join(&mut keys.iter().map(|e| e.key)),
            join(&mut v.iter().map(|e| e.id)),
            calls.len() / 2
        );
        if n <= 12 {
            got += &format!(" | {}", join(&mut calls.iter().copied()));
        }
        assert_eq!(got, line, "sort of {n} keys");
    }
}

#[test]
fn priority_queue_pops_as_std_priority_queue() {
    // std::priority_queue<int> pushing 3 1 4 1 5 9 2 6 pops 9 6 5 4 3 2 1 1, and the two equal
    // keys of a priority_queue of pairs ordered by first come out as push_heap and pop_heap put
    // them: the heap of the eight pushes is [9 6 5 4 1 1 2 3] (std::make_heap's order).
    let mut q = PriorityQueue::new();
    for x in [3, 1, 4, 1, 5, 9, 2, 6] {
        q.push(x);
    }
    let mut v = vec![3, 1, 4, 1, 5, 9, 2, 6];
    for k in 1..=v.len() {
        push_heap(&mut v[..k]);
    }
    assert_eq!(q.as_slice(), v.as_slice());
    let mut out = Vec::new();
    while let Some(x) = q.pop() {
        out.push(x);
    }
    assert_eq!(out, [9, 6, 5, 4, 3, 2, 1, 1]);
}

#[test]
fn min_max_and_clamp_follow_msvc_on_nans_and_zeros() {
    let vals = [
        f64::NAN,
        -f64::NAN,
        -0.0,
        0.0,
        1.0,
        -1.0,
        f64::INFINITY,
        f64::NEG_INFINITY,
        2.5,
    ];
    let mut lines = Lines::new();
    for a in vals {
        for b in vals {
            let ordered = a < b || a.is_nan() || b.is_nan();
            let (lo, hi) = if ordered { (a, b) } else { (b, a) };
            lines.push(format!(
                "minmax {:016x} {:016x} max={:016x} min={:016x} clamp={:016x}",
                a.to_bits(),
                b.to_bits(),
                max(a, b).to_bits(),
                min(a, b).to_bits(),
                clamp(2.0, lo, hi).to_bits()
            ));
        }
    }
    lines.has(
        "minmax 7ff0000000000000 8000000000000000 max=7ff0000000000000 min=8000000000000000 clamp=4000000000000000",
    );
    lines.has(
        "minmax 8000000000000000 0000000000000000 max=8000000000000000 min=8000000000000000 clamp=8000000000000000",
    );
    lines.check("minmax", MINMAX_LINES.0, MINMAX_LINES.1);
}

#[test]
fn element_searches_follow_msvcs_vectorised_lanes() {
    let mut lines = Lines::new();
    for n in 1..=70i32 {
        for variant in 0..6 {
            let mut sm = SplitMix(n as u64 * 31 + variant as u64);
            let mut v: Vec<f64> = (0..n)
                .map(|_| f64::from((sm.next() % 9) as i32) - 4.0)
                .collect();
            let len = v.len();
            match variant {
                1 => v[0] = f64::NAN,
                2 => v[len / 2] = f64::NAN,
                3 => v[len - 1] = f64::NAN,
                4 => {
                    for x in &mut v {
                        *x = if sm.next() & 1 != 0 { 0.0 } else { -0.0 };
                    }
                }
                5 => {
                    for x in v.iter_mut().step_by(3) {
                        *x = f64::NAN;
                    }
                }
                _ => {}
            }
            let mx = max_element_f64(&v);
            let mn = min_element_f64(&v);
            let (mlo, mhi) = minmax_element_f64(&v);
            if variant == 0 {
                // Without NaNs: the plain loops' answers.
                assert_eq!(mx, max_element_by(&v, |a, b| a < b));
                assert_eq!(mn, min_element_by(&v, |a, b| a < b));
                assert_eq!((mlo, mhi), minmax_element_by(&v, |a, b| a < b));
            }
            lines.push(format!(
                "elements {n} {variant} max={mx} min={mn} maxi={mx} mini={mn} mm={mlo},{mhi}"
            ));
        }
    }
    lines.check("elements", ELEMENTS_LINES.0, ELEMENTS_LINES.1);

    let nan = f64::NAN;
    assert_eq!(
        max_of_f64(&[nan, 1.0, 2.0]).to_bits(),
        0x4000_0000_0000_0000
    );
    assert_eq!(
        max_of_f64(&[1.0, nan, 2.0]).to_bits(),
        0x4000_0000_0000_0000
    );
    assert_eq!(
        min_of_f64(&[nan, 1.0, 2.0]).to_bits(),
        0x4000_0000_0000_0000
    );
    assert_eq!(
        min_of_f64(&[-0.0, 0.0, 0.0]).to_bits(),
        0x8000_0000_0000_0000
    );
    assert_eq!(
        max_of_f64(&[nan, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]).to_bits(),
        0x7ff8_0000_0000_0000
    );
    assert_eq!(
        min_of_f64(&[0.0, -0.0, 1.0, 2.0, 3.0, 4.0, 5.0, -0.0]).to_bits(),
        0
    );
    let f: Vec<f32> = std::iter::once(f32::NAN)
        .chain((1..16).map(|i| i as f32))
        .collect();
    assert_eq!(max_of_f32(&f).to_bits(), 0x7fc0_0000);
    assert_eq!((max_of(&[3, 9, -2, 9]), min_of(&[3, 9, -2, -2])), (9, -2));
    assert_eq!(max_of(&[1i64, 5, 2, 7, 7, 0, -1, 6, 3]), 7);
}

#[test]
fn std_hash_is_fnv1a_over_the_bytes() {
    let h = |v: u64| format!("{v:x}");
    assert_eq!(
        [std_hash(&0i32), std_hash(&1i32), std_hash(&-123_456_789i32)].map(h),
        ["4d25767f9dce13f5", "ad2aca7747985764", "796832016a16ba94"]
    );
    assert_eq!(
        [std_hash(&0i64), std_hash(&-5_000_000_000i64)].map(h),
        ["a8c7f832281a39c5", "bae9b61253411877"]
    );
    assert_eq!(h(std_hash(&4_000_000_000u32)), "1d94ee8014055b3c");
    assert_eq!(h(std_hash(&0xFEDC_BA98_7654_3210u64)), "d38edce00b234935");
    assert_eq!(h(std_hash(&12345usize)), "e71eb185e2edcc4c");
    assert_eq!(
        [std_hash(&-2i16), std_hash(&200u8), std_hash(&-3i8)].map(h),
        ["a9dc907b6fa303a", "af64854c86030bb7", "af64704c8602e808"]
    );
    assert_eq!(
        [std_hash(&false), std_hash(&true)].map(h),
        ["af63bd4c8601b7df", "af63bc4c8601b62c"]
    );
    assert_eq!(
        [
            std_hash(&0.0f64),
            std_hash(&-0.0f64),
            std_hash(&1.5f64),
            std_hash(&-2.25e300f64),
            std_hash(&f64::NAN)
        ]
        .map(h),
        [
            "a8c7f832281a39c5",
            "a8c7f832281a39c5",
            "aa95e93229a27c80",
            "442029804e882289",
            "aa96293229a2e940"
        ]
    );
    assert_eq!(
        [std_hash(&0.0f32), std_hash(&-0.0f32), std_hash(&3.75f32)].map(h),
        ["4d25767f9dce13f5", "4d25767f9dce13f5", "4c15567f9ce6aa65"]
    );
    assert_eq!(
        [
            std_hash(""),
            std_hash("a"),
            std_hash(&String::from("Sketch slices with Orca"))
        ]
        .map(h),
        ["cbf29ce484222325", "af63dc4c8601ec8c", "828872ec24d01909"]
    );
    assert_eq!(hash_bytes(b"a"), std_hash("a"));
}

fn order_ints<'a>(it: impl Iterator<Item = (&'a i32, &'a i32)>) -> u64 {
    let mut f = Fnv::new();
    for (k, v) in it {
        f.u64(i64::from(*k) as u64);
        f.u64(i64::from(*v) as u64);
    }
    f.0
}

#[test]
fn unordered_map_keeps_msvcs_order_through_every_operation() {
    let mut lines = Lines::new();
    for n in [
        0i32, 1, 5, 8, 9, 10, 63, 64, 65, 100, 511, 512, 513, 1000, 4097, 20000,
    ] {
        for range in [16u64, 1000, 1 << 31] {
            let mut sm = SplitMix(n as u64 * 7 + range);
            let quarter = (range / 4) as i32;
            let mut m: UnorderedMap<i32, i32> = UnorderedMap::new();
            for i in 0..n {
                let k = (sm.next() % range) as i32 - quarter;
                match i % 3 {
                    0 => *m.get_or_insert_default(k) += i,
                    1 => {
                        m.insert(k, i);
                    }
                    _ => {
                        m.insert(k, -i);
                    }
                }
            }
            let a = order_ints(m.iter());
            let b0 = m.bucket_count();
            for _ in 0..n / 3 {
                let k = (sm.next() % range) as i32 - quarter;
                m.remove(&k);
            }
            let b = order_ints(m.iter());
            let copy = m.clone();
            let c = order_ints(copy.iter());
            let mut assigned: UnorderedMap<i32, i32> = UnorderedMap::new();
            *assigned.get_or_insert_default(1) = 2;
            assigned.clone_from(&m);
            let d = order_ints(assigned.iter());
            assert!(assigned == m && copy == m);
            m.reserve(n as usize * 3 + 1);
            let e = order_ints(m.iter());
            let be = m.bucket_count();
            m.rehash(3);
            m.clear();
            for i in 0..n / 2 {
                *m.get_or_insert_default((sm.next() % range) as i32) = i;
            }
            let g = order_ints(m.iter());
            for bucket in 0..m.bucket_count() {
                assert!(m.bucket_iter(bucket).all(|(k, _)| m.bucket(k) == bucket));
            }
            lines.push(format!(
                "umap {n} {range} b0={b0} a={a:016x} b={b:016x} copy={c:016x}/{} assigned={d:016x}/{} reserved={e:016x}/{be} cleared={g:016x}/{} size={}",
                copy.bucket_count(),
                assigned.bucket_count(),
                m.bucket_count(),
                m.len()
            ));
        }
    }
    lines.check("umap", UMAP_LINES.0, UMAP_LINES.1);

    let keys = |m: &UnorderedMap<i32, i32>| {
        m.keys()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut m = UnorderedMap::new();
    for k in [5, 3, 12, 8, 1, 21, 13, 34, 55, 2, 89, 144, 0, -1, 7] {
        *m.get_or_insert_default(k) = k * k;
    }
    assert_eq!(
        (keys(&m), m.bucket_count()),
        ("13 21 5 3 12 8 1 34 55 2 89 144 0 -1 7".into(), 64)
    );
    m.remove(&13);
    m.remove(&5);
    *m.get_or_insert_default(100) = 1;
    assert_eq!(
        (keys(&m), m.bucket_count()),
        ("21 3 12 8 1 34 55 2 89 144 0 100 -1 7".into(), 64)
    );
    let c = m.clone();
    assert_eq!(
        (keys(&c), c.bucket_count()),
        ("21 3 12 8 1 34 55 2 89 144 0 -1 100 7".into(), 64)
    );

    let mut m = UnorderedMap::new();
    m.set_max_load_factor(0.5);
    for i in 0..300 {
        *m.get_or_insert_default(i * 37 % 1001) = i;
    }
    assert_eq!(
        (format!("{:016x}", order_ints(m.iter())), m.bucket_count()),
        ("1aca3250e9a090d2".into(), 1024)
    );
    let mut m = UnorderedMap::with_buckets(100);
    assert_eq!(m.bucket_count(), 128);
    for i in 0..300 {
        *m.get_or_insert_default(i * 37 % 1001) = i;
    }
    assert_eq!(
        (format!("{:016x}", order_ints(m.iter())), m.bucket_count()),
        ("9c6b8fc97e25f056".into(), 1024)
    );
}

#[test]
fn unordered_sets_keep_msvcs_order() {
    let mut s = UnorderedSet::new();
    for w in [
        "wall", "infill", "skirt", "brim", "support", "ironing", "wall", "bridge", "gap fill",
        "top", "bottom", "travel", "retract", "wipe", "seam",
    ] {
        s.insert(w.to_string());
    }
    let order: Vec<String> = s.iter().map(|w| format!("[{w}]")).collect();
    assert_eq!(
        (order.join(" "), s.bucket_count()),
        (
            "[ironing] [wall] [brim] [seam] [infill] [bridge] [gap fill] [support] [skirt] [top] [bottom] [travel] [retract] [wipe]"
                .into(),
            64
        )
    );
    assert!(s.contains("seam") && !s.contains("perimeter"));

    let mut d = UnorderedSet::new();
    for x in [0.5, -0.0, 0.0, 1.25, 3.0, -7.5, 1e100, 2.0, 0.1, 0.2, 0.3] {
        d.insert(x);
    }
    let bits: Vec<String> = d
        .iter()
        .map(|x: &f64| format!("{:016x}", x.to_bits()))
        .collect();
    assert_eq!(
        (bits.join(" "), d.bucket_count()),
        (
            "3fe0000000000000 8000000000000000 4000000000000000 4008000000000000 3fb999999999999a 3ff4000000000000 c01e000000000000 54b249ad2594c37d 3fc999999999999a 3fd3333333333333"
                .into(),
            64
        )
    );

    let expect = [
        (10u64, "9a59120591bf1623", 7, 8),
        (100, "64bf3db76c461652", 68, 512),
        (1000, "adf310b67cd35a7c", 686, 1024),
        (10000, "7a11abdbb5f774ab", 6949, 8192),
    ];
    for (n, digest, size, buckets) in expect {
        let mut sm = SplitMix(n * 13);
        let mut s = UnorderedSet::new();
        for _ in 0..n {
            s.insert(sm.next() % (n * 2));
        }
        for _ in 0..n / 4 {
            s.remove(&(sm.next() % (n * 2)));
        }
        let mut f = Fnv::new();
        for x in &s {
            f.u64(*x);
        }
        assert_eq!(
            (format!("{:016x}", f.0), s.len(), s.bucket_count()),
            (digest.into(), size, buckets),
            "{n}"
        );
    }
}

#[test]
fn unordered_map_holds_what_a_map_holds() {
    // 20000 random inserts, assignments, lookups and erases against a BTreeMap: the same contents,
    // the bucket of every element its hash's, and the load factor at most the maximum.
    let mut sm = SplitMix(2026);
    let mut m: UnorderedMap<u64, u64> = UnorderedMap::new();
    let mut reference = std::collections::BTreeMap::new();
    for i in 0..20_000u64 {
        let k = sm.next() % 3000;
        match sm.next() % 4 {
            0 => {
                let fresh = !reference.contains_key(&k);
                assert_eq!(m.insert(k, i), fresh);
                reference.entry(k).or_insert(i);
            }
            1 => {
                m.insert_or_assign(k, i);
                reference.insert(k, i);
            }
            2 => assert_eq!(m.remove(&k), reference.remove(&k)),
            _ => assert_eq!(m.get(&k), reference.get(&k)),
        }
        assert!(m.load_factor() <= m.max_load_factor());
    }
    assert_eq!(m.len(), reference.len());
    let mut items: Vec<(u64, u64)> = m.iter().map(|(k, v)| (*k, *v)).collect();
    sort(&mut items);
    assert_eq!(items, reference.into_iter().collect::<Vec<_>>());
    let total: usize = (0..m.bucket_count()).map(|b| m.bucket_size(b)).sum();
    assert_eq!(total, m.len());

    // retain walks in order and erases as it goes; values_mut and into_iter keep the order.
    let order: Vec<u64> = m.keys().copied().filter(|k| k % 2 == 0).collect();
    m.retain(|k, _| k % 2 == 0);
    assert_eq!(m.keys().copied().collect::<Vec<_>>(), order);
    for v in m.values_mut() {
        *v = 0;
    }
    assert!(m.values().all(|v| *v == 0));
    assert_eq!(m.into_iter().map(|(k, _)| k).collect::<Vec<_>>(), order);

    // String keys are found through a &str, with the same hash.
    let mut s: UnorderedMap<String, i32> = UnorderedMap::new();
    s.insert("wall".into(), 1);
    assert_eq!(s.get("wall"), Some(&1));
    assert_eq!(s.bucket("wall"), s.bucket(&String::from("wall")));
    assert!(s.contains_key("wall") && s.count("brim") == 0);
}

#[test]
fn element_searches_agree_with_the_plain_loops_without_nans() {
    let mut sm = SplitMix(7);
    for n in 0..100usize {
        let v: Vec<f64> = (0..n)
            .map(|_| f64::from((sm.next() % 7) as i32) - 3.0)
            .collect();
        let w: Vec<f32> = v.iter().map(|&x| x as f32).collect();
        let lt64 = |a: &f64, b: &f64| a < b;
        let lt32 = |a: &f32, b: &f32| a < b;
        for isa in [
            minmax::VectorIsa::Scalar,
            minmax::VectorIsa::Sse42,
            minmax::VectorIsa::Avx2,
        ] {
            if n == 0 {
                continue;
            }
            assert_eq!(
                minmax::max_element_f64_with(&v, isa),
                max_element_by(&v, lt64)
            );
            assert_eq!(
                minmax::min_element_f64_with(&v, isa),
                min_element_by(&v, lt64)
            );
            assert_eq!(
                minmax::minmax_element_f64_with(&v, isa),
                minmax_element_by(&v, lt64)
            );
            assert_eq!(
                minmax::max_element_f32_with(&w, isa),
                max_element_by(&w, lt32)
            );
            assert_eq!(
                minmax::min_element_f32_with(&w, isa),
                min_element_by(&w, lt32)
            );
            assert_eq!(
                minmax::minmax_element_f32_with(&w, isa),
                minmax_element_by(&w, lt32)
            );
        }
    }
}

#[test]
fn mersenne_twisters_give_the_recorded_sequences() {
    let expect32 = [
        (
            0u32,
            [
                2_357_136_044u32,
                2_546_248_239,
                3_071_714_933,
                3_626_093_760,
                2_588_848_963,
            ],
            1_543_171_712u32,
        ),
        (
            1,
            [
                1_791_095_845,
                4_282_876_139,
                3_093_770_124,
                4_005_303_368,
                491_263,
            ],
            1_237_896_635,
        ),
        (
            42,
            [
                1_608_637_542,
                3_421_126_067,
                4_083_286_876,
                787_846_414,
                3_143_890_026,
            ],
            1_399_405_940,
        ),
        (
            5489,
            [
                3_499_211_612,
                581_869_302,
                3_890_346_734,
                3_586_334_585,
                545_404_204,
            ],
            4_123_659_995,
        ),
        (
            u32::MAX,
            [
                419_326_371,
                479_346_978,
                3_918_654_476,
                2_416_749_639,
                3_388_880_820,
            ],
            1_117_955_853,
        ),
        (
            123_456_789,
            [
                2_288_500_408,
                4_254_805_660,
                2_294_099_250,
                56_498_137,
                2_188_513_626,
            ],
            4_147_794_090,
        ),
    ];
    for (seed, first, ten_thousandth) in expect32 {
        let mut g = Mt19937::new(seed);
        assert_eq!([(); 5].map(|_| g.next()), first);
        g.discard(9994);
        assert_eq!(g.next(), ten_thousandth);
    }
    let expect64 = [
        (
            0u64,
            [
                2_947_667_278_772_165_694u64,
                18_301_848_765_998_365_067,
                729_919_693_006_235_833,
            ],
            16_335_088_777_103_562_557u64,
        ),
        (
            1,
            [
                2_469_588_189_546_311_528,
                2_516_265_689_700_432_462,
                8_323_445_853_463_659_930,
            ],
            12_541_479_624_422_949_620,
        ),
        (
            42,
            [
                13_930_160_852_258_120_406,
                11_788_048_577_503_494_824,
                13_874_630_024_467_741_450,
            ],
            9_487_037_760_323_427_527,
        ),
        (
            5489,
            [
                14_514_284_786_278_117_030,
                4_620_546_740_167_642_908,
                13_109_570_281_517_897_720,
            ],
            9_981_545_732_273_789_042,
        ),
        (
            4_294_967_295,
            [
                3_814_183_646_661_098_318,
                15_337_012_084_077_158_743,
                15_360_522_117_559_280_555,
            ],
            6_169_176_550_437_654_027,
        ),
        (
            123_456_789,
            [
                6_435_547_048_506_935_310,
                4_923_172_384_746_461_813,
                2_520_679_223_035_091_359,
            ],
            13_513_797_969_445_696_817,
        ),
    ];
    for (seed, first, ten_thousandth) in expect64 {
        let mut h = Mt19937_64::new(seed);
        assert_eq!([(); 3].map(|_| h.next()), first);
        h.discard(9996);
        assert_eq!(h.next(), ten_thousandth);
    }
    let mut g = DefaultRandomEngine::default();
    g.discard(9999);
    assert_eq!(g.next(), 4_123_659_995);
}

#[test]
fn seed_sequences_give_the_recorded_states() {
    let mut out = [0u32; 10];
    SeedSeq::new([7, 8, 9, 10, 11, 12, 13, 14]).generate(&mut out);
    assert_eq!(
        out,
        [
            709_213_818,
            478_867_093,
            1_390_415_154,
            136_144_512,
            380_345_006,
            14_755_132,
            2_051_317_906,
            1_317_463_121,
            1_255_807_033,
            2_990_787_726
        ]
    );
    // The recording program printed its five calls as printf arguments, which MSVC evaluates
    // right to left: the first number printed is each engine's last call.
    let mut g = Mt19937::from_seed_seq(&SeedSeq::new([1, 2, 3]));
    let mut h = Mt19937_64::from_seed_seq(&SeedSeq::new([0x1234_5678, 0x9abc_def0]));
    let mut z = Mt19937::from_seed_seq(&SeedSeq::default());
    let z1 = z.next();
    let (h1, h2) = (h.next(), h.next());
    let (g1, g2) = (g.next(), g.next());
    assert_eq!(
        format!("{g2} {g1} {h2} {h1} {z1}"),
        "703781052 1710881851 10625845634660318398 11261588490880110052 2872601305"
    );
}

#[test]
fn distributions_draw_the_recorded_numbers() {
    let mut g = Mt19937::new(7);
    let mut f = Fnv::new();
    let (d1, d2, d3) = (
        UniformIntDistribution::new(0i32, 9),
        UniformIntDistribution::new(-5i32, 5),
        UniformIntDistribution::new(0, i32::MAX),
    );
    let (d4, d5) = (
        UniformIntDistribution::new(i32::MIN, i32::MAX),
        UniformIntDistribution::new(100i32, 100),
    );
    for _ in 0..2000 {
        for d in [&d1, &d2, &d3, &d4, &d5] {
            let x = d.sample(&mut g);
            assert!(d.a() <= x && x <= d.b());
            f.u32(x as u32);
        }
    }
    let (e1, e2) = (
        UniformIntDistribution::new(-3i64, 1 << 40),
        UniformIntDistribution::new(i64::MIN, i64::MAX),
    );
    let (e3, e4) = (
        UniformIntDistribution::new(0u64, 6),
        UniformIntDistribution::new(1u64, (1 << 63) + 5),
    );
    let e5 = UniformIntDistribution::new(0usize, 1_000_000);
    let e6 = UniformIntDistribution::new(-300i16, 300);
    let e7 = UniformIntDistribution::new(0u32, 0xFFFF_FFFE);
    for _ in 0..2000 {
        f.u64(e1.sample(&mut g) as u64);
        f.u64(e2.sample(&mut g) as u64);
        f.u64(e3.sample(&mut g));
        f.u64(e4.sample(&mut g));
        f.u64(e5.sample(&mut g) as u64);
        f.u32(i32::from(e6.sample(&mut g)) as u32);
        f.u32(e7.sample(&mut g));
    }
    assert_eq!(
        (format!("{:016x}", f.0), g.next()),
        ("d761b8fcf3f21398".into(), 1_105_017_042)
    );

    let mut g = Mt19937_64::new(7);
    let mut f = Fnv::new();
    for _ in 0..2000 {
        f.u32(d1.sample(&mut g) as u32);
        f.u32(d4.sample(&mut g) as u32);
        f.u64(e1.sample(&mut g) as u64);
        f.u64(e2.sample(&mut g) as u64);
        f.u64(e4.sample(&mut g));
    }
    assert_eq!(
        (format!("{:016x}", f.0), g.next()),
        ("6cedee7b75cbbdf0".into(), 11_462_758_112_057_870_171)
    );

    let mut g = Mt19937::new(11);
    let mut h = Mt19937_64::new(11);
    let mut f = Fnv::new();
    let (a, b) = (
        UniformRealDistribution::new(0.0f64, 1.0),
        UniformRealDistribution::new(-3.5f64, 2.25),
    );
    let c = UniformRealDistribution::<f64>::default();
    let (d, e) = (
        UniformRealDistribution::new(0.0f32, 1.0),
        UniformRealDistribution::new(-100.0f32, 0.001),
    );
    for _ in 0..3000 {
        f.u64(a.sample(&mut g).to_bits());
        let y = b.sample(&mut g);
        assert!((-3.5..2.25).contains(&y));
        f.u64(y.to_bits());
        f.u64(c.sample(&mut h).to_bits());
        let x = d.sample(&mut g);
        let y = e.sample(&mut h);
        f.u32(x.to_bits());
        f.u32(y.to_bits());
        f.u64(generate_canonical_f64(&mut g, 53).to_bits());
        f.u32(generate_canonical_f32(&mut h, 24).to_bits());
    }
    assert_eq!(
        (format!("{:016x}", f.0), g.next(), h.next()),
        (
            "c56381f3ff68a8e3".into(),
            3_527_450_091,
            8_562_664_449_330_361_779
        )
    );
    let mut g = Mt19937::new(3);
    let u01 = UniformRealDistribution::<f64>::default();
    assert_eq!(
        [(); 4].map(|_| format!("{:016x}", u01.sample(&mut g).to_bits())),
        [
            "3fb21b06988d0110",
            "3feae0dcd076a925",
            "3fbf0f63c84a78b8",
            "3fe237cc62b058b3"
        ]
    );
}

#[test]
fn shuffle_permutes_as_msvc() {
    let expect = [
        (
            0,
            "cbf29ce484222325",
            "cbf29ce484222325",
            1_791_095_845u32,
            2_469_588_189_546_311_528u64,
        ),
        (
            1,
            "4d25767f9dce13f5",
            "4d25767f9dce13f5",
            1_872_583_848,
            16_668_552_215_174_154_828,
        ),
        (
            2,
            "89cd31291d2aefa4",
            "08cd4c29d1e47d34",
            303_761_048,
            3_611_203_882_987_592_167,
        ),
        (
            3,
            "9ef40c127c771966",
            "756241e1be8c9396",
            2_350_344_631,
            10_961_989_281_185_213_082,
        ),
        (
            10,
            "8d3e16bd9903a004",
            "169d96a388ec7c74",
            451_723_847,
            16_084_216_986_712_066_191,
        ),
        (
            100,
            "ec57419af40ca575",
            "478965639df190a5",
            2_650_973_978,
            10_911_084_726_118_899_398,
        ),
        (
            1000,
            "282015f6310d3ddd",
            "06a8c4db18d73c49",
            3_497_183_283,
            468_726_692_275_340_606,
        ),
        (
            5000,
            "b60f5a1fbdca44ed",
            "131d7bb56fe678f9",
            377_946_153,
            10_671_260_701_941_453_883,
        ),
    ];
    for (n, d32, d64, next32, next64) in expect {
        let digest = |v: &[i32]| {
            let mut f = Fnv::new();
            for &x in v {
                f.u32(x as u32);
            }
            format!("{:016x}", f.0)
        };
        let mut v: Vec<i32> = (0..n).collect();
        let mut g = Mt19937::new(n as u32 + 1);
        shuffle(&mut v, &mut g);
        let mut w: Vec<i32> = (0..n).collect();
        let mut h = Mt19937_64::new(n as u64 + 1);
        shuffle(&mut w, &mut h);
        assert_eq!(
            (digest(&v), digest(&w), g.next(), h.next()),
            (d32.into(), d64.into(), next32, next64),
            "{n}"
        );
        let mut sorted = v.clone();
        sort(&mut sorted);
        assert_eq!(sorted, (0..n).collect::<Vec<_>>());
    }
    let mut v: Vec<i32> = (0..10).collect();
    shuffle(&mut v, &mut Mt19937::new(2026));
    assert_eq!(v, [9, 4, 3, 5, 6, 7, 1, 8, 0, 2]);
}

/// The recorded lines of each kind: (number of lines, FNV-1a digest of the lines).
const SORT_LINES: (usize, u64) = (468, 0x0908_e3ba_d9ca_6477);
const STABLE_SORT_LINES: (usize, u64) = (468, 0xfb7d_8482_36f4_993d);
const STABLE_SORT_CAP_LINES: (usize, u64) = (3024, 0xf41e_49c7_a412_418c);
const NTH_ELEMENT_LINES: (usize, u64) = (2784, 0x063d_7000_e043_c003);
const PARTIAL_SORT_LINES: (usize, u64) = (2328, 0x6f24_404a_3031_6eae);
const INPLACE_MERGE_LINES: (usize, u64) = (468, 0x952f_fdc4_6ebc_0afe);
const HEAP_LINES: (usize, u64) = (468, 0xe91e_0b0b_1749_2ced);
const BOUNDS_LINES: (usize, u64) = (468, 0xe488_56bd_c43d_7aa9);
const UNIQUE_LINES: (usize, u64) = (468, 0x9eb2_0c00_c8de_0c3e);
const MINMAX_LINES: (usize, u64) = (81, 0x1b3f_84e5_e1ce_289b);
const ELEMENTS_LINES: (usize, u64) = (420, 0x46eb_be3d_0706_ac28);
const UMAP_LINES: (usize, u64) = (48, 0x419e_8446_4af8_727d);
