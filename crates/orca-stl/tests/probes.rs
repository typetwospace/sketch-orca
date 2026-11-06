// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-stl: every record of `stl.sop1` (`parity/SOP1.md`, "Probe files"),
//! which the probe program wrote with the C++ engine's own compile and link lines, replayed
//! against the crate.
//!
//! - SORT: each algorithm on the recorded input with a comparator that logs its calls: the same
//!   calls in the same order, the same result and the same returned positions.
//! - VALS: `min`, `max`, `clamp` and the element searches on NaNs and signed zeros: the bits.
//! - HASH: `std::hash` of each type.
//! - UMAP and USET: the scripts' iteration orders, bucket counts and sizes at each snapshot.
//! - RNGS, SSEQ and SHUF: every draw, the generated words, the shuffled order and the next value.
//!
//! The committed subset (`parity/probes/`) runs by default; `SKETCH_ORCA_PROBES_DEEP=<folder>`
//! replays the full set from that folder (`<work>/parity/probes`).

use std::cell::RefCell;

use orca_probe::{Mismatches, ProbeFile, Reader, load_probe};
use orca_stl::minmax::{max_of, max_of_f64, min_of, min_of_f64};
use orca_stl::unordered::hash_bytes;
use orca_stl::{
    Mt19937, Mt19937_64, PriorityQueue, SeedSeq, UniformIntDistribution, UniformRealDistribution,
    UnorderedMap, UnorderedSet, Urbg,
};

fn file() -> ProbeFile {
    load_probe("stl").unwrap_or_else(|e| panic!("{e}"))
}

#[allow(clippy::cast_possible_truncation)]
fn f32_of(x: u64) -> f32 {
    f32::from_bits(x as u32)
}

fn f32_bits(x: f32) -> u64 {
    u64::from(x.to_bits())
}

#[allow(clippy::cast_sign_loss)]
fn signed(x: i64) -> u64 {
    x as u64
}

fn index(i: usize) -> u64 {
    u64::try_from(i).expect("an index below 2^64")
}

fn size(i: i64) -> usize {
    usize::try_from(i).expect("a size the record keeps non-negative")
}

// ---------------------------------------------------------------- SORT

/// An element: its key and its id, as the probe's `E` and `D`.
#[derive(Clone, Copy, Debug)]
struct El<K> {
    key: K,
    id: i32,
}

/// What a SORT record holds after its input.
#[derive(Debug, PartialEq)]
struct SortOutcome {
    calls: Vec<i32>,
    result: Vec<i32>,
    extra: Vec<i64>,
}

fn ids<K>(v: &[El<K>]) -> Vec<i32> {
    v.iter().map(|e| e.id).collect()
}

/// Runs one SORT record's algorithm with a comparator `a.key < b.key` that logs each call's ids.
fn run_sort<K: Copy + PartialOrd + From<i32>>(
    algo: &str,
    params: &[i64],
    input: &[El<K>],
) -> SortOutcome {
    let log = RefCell::new(Vec::new());
    let less = |a: &El<K>, b: &El<K>| {
        log.borrow_mut().extend([a.id, b.id]);
        a.key < b.key
    };
    let mut v = input.to_vec();
    let mut result = Vec::new();
    let mut extra = Vec::new();
    match algo {
        "sort" => orca_stl::sort_by(&mut v, less),
        "stable_sort" => orca_stl::stable_sort_by(&mut v, less),
        "stable_sort_cap" => orca_stl::stable_sort_by_with_capacity(&mut v, size(params[0]), less),
        "nth_element" => orca_stl::nth_element_by(&mut v, size(params[0]), less),
        "partial_sort" => orca_stl::partial_sort_by(&mut v, size(params[0]), less),
        "inplace_merge" => orca_stl::inplace_merge_by(&mut v, size(params[0]), less),
        "heap" => {
            let until = orca_stl::is_heap_until_by(&v, less);
            orca_stl::make_heap_by(&mut v, less);
            result.extend(ids(&v));
            for k in (2..=v.len()).rev() {
                orca_stl::pop_heap_by(&mut v[..k], less);
            }
            result.extend(ids(&v));
            for k in 1..=v.len() {
                orca_stl::push_heap_by(&mut v[..k], less);
            }
            orca_stl::sort_heap_by(&mut v, less);
            result.extend(ids(&v));
            extra.push(i64::try_from(until).unwrap());
        }
        "priority_queue" => {
            let mut q = PriorityQueue::with_compare(less);
            for e in input {
                q.push(*e);
            }
            while let Some(top) = q.top() {
                result.push(top.id);
                q.pop();
            }
        }
        "bounds" => {
            for &key in params {
                let probe = El {
                    key: K::from(i32::try_from(key).unwrap()),
                    id: -1,
                };
                extra.push(i64::try_from(orca_stl::lower_bound_by(&v, &probe, less)).unwrap());
                extra.push(i64::try_from(orca_stl::upper_bound_by(&v, &probe, less)).unwrap());
            }
        }
        "unique" => {
            let end = orca_stl::unique_by(&mut v, |a, b| {
                log.borrow_mut().extend([a.id, b.id]);
                a.key == b.key
            });
            v.truncate(end);
            extra.push(i64::try_from(end).unwrap());
        }
        _ => panic!("a SORT record for {algo}"),
    }
    if !matches!(algo, "heap" | "priority_queue") {
        result = ids(&v);
    }
    SortOutcome {
        calls: log.into_inner(),
        result,
        extra,
    }
}

#[test]
fn sorts_make_msvcs_comparator_calls_and_results() {
    let file = file();
    let mut m = Mismatches::default();
    for (n, rec) in file.of_kind("SORT").enumerate() {
        let mut r = rec.reader();
        let algo = r.str().unwrap();
        let key_type = r.u8().unwrap();
        let params = r.vec(Reader::i64).unwrap();
        let keys = r.vec(Reader::u64).unwrap();
        let input_ids = r.vec(Reader::i32).unwrap();
        let want = SortOutcome {
            calls: r.vec(Reader::i32).unwrap(),
            result: r.vec(Reader::i32).unwrap(),
            extra: r.vec(Reader::i64).unwrap(),
        };
        r.finish("SORT").unwrap();
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let got = if key_type == 0 {
            let input: Vec<El<i32>> = keys
                .iter()
                .zip(&input_ids)
                .map(|(&k, &id)| El {
                    key: k as i64 as i32,
                    id,
                })
                .collect();
            run_sort(&algo, &params, &input)
        } else {
            let input: Vec<El<f64>> = keys
                .iter()
                .zip(&input_ids)
                .map(|(&k, &id)| El {
                    key: f64::from_bits(k),
                    id,
                })
                .collect();
            run_sort(&algo, &params, &input)
        };
        m.check(got == want, || {
            let first_call = got.calls.iter().zip(&want.calls).position(|(a, b)| a != b);
            format!(
                "SORT #{n} {algo} keys {key_type} n {} params {params:?}: {} calls (MSVC {}), first differing call value at {first_call:?}; result same {}; extra {:?} (MSVC {:?})",
                keys.len(),
                got.calls.len() / 2,
                want.calls.len() / 2,
                got.result == want.result,
                got.extra,
                want.extra
            )
        });
    }
    m.assert_none("SORT");
}

// ---------------------------------------------------------------- VALS

fn vals(name: &str, input: &[u64]) -> Vec<u64> {
    match name {
        "max-min-clamp-f64" => {
            let [a, b, lo, hi] = [0, 1, 2, 3].map(|i| f64::from_bits(input[i]));
            vec![
                orca_stl::max(a, b).to_bits(),
                orca_stl::min(a, b).to_bits(),
                orca_stl::clamp(2.0, lo, hi).to_bits(),
            ]
        }
        "max-min-f32" => {
            let (a, b) = (f32_of(input[0]), f32_of(input[1]));
            vec![f32_bits(orca_stl::max(a, b)), f32_bits(orca_stl::min(a, b))]
        }
        "elements-f64" => {
            let v: Vec<f64> = input.iter().map(|&x| f64::from_bits(x)).collect();
            // The pointer range and the vector's iterators both reach the vectorised search.
            let (lo, hi) = orca_stl::minmax_element_f64(&v);
            vec![
                index(orca_stl::max_element_f64(&v)),
                index(orca_stl::min_element_f64(&v)),
                index(orca_stl::max_element_f64(&v)),
                index(orca_stl::min_element_f64(&v)),
                index(lo),
                index(hi),
            ]
        }
        "initializer-list-f64" => {
            let nan = f64::NAN;
            vec![
                max_of_f64(&[nan, 1.0, 2.0]).to_bits(),
                max_of_f64(&[1.0, nan, 2.0]).to_bits(),
                min_of_f64(&[nan, 1.0, 2.0]).to_bits(),
                min_of_f64(&[-0.0, 0.0, 0.0]).to_bits(),
                max_of_f64(&[nan, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]).to_bits(),
                min_of_f64(&[0.0, -0.0, 1.0, 2.0, 3.0, 4.0, 5.0, -0.0]).to_bits(),
            ]
        }
        "initializer-list-int" => vec![
            signed(i64::from(max_of(&[3, 9, -2, 9]))),
            signed(i64::from(min_of(&[3, 9, -2, -2]))),
            signed(max_of(&[1i64, 5, 2, 7, 7, 0, -1, 6, 3])),
        ],
        _ => panic!("a VALS record for {name}"),
    }
}

#[test]
fn min_max_and_element_searches_give_msvcs_bits() {
    let file = file();
    let mut m = Mismatches::default();
    for rec in file.of_kind("VALS") {
        let mut r = rec.reader();
        let name = r.str().unwrap();
        let input = r.vec(Reader::u64).unwrap();
        let want = r.vec(Reader::u64).unwrap();
        r.finish("VALS").unwrap();
        let got = vals(&name, &input);
        m.check(got == want, || {
            format!("{name} of {input:#x?} gave {got:#x?}, MSVC {want:#x?}")
        });
    }
    m.assert_none("VALS");
}

// ---------------------------------------------------------------- HASH

fn hash_of(ty: &str, bytes: &[u8]) -> u64 {
    fn arr<const N: usize>(b: &[u8]) -> [u8; N] {
        b.try_into().expect("the value's bytes")
    }
    match ty {
        "i32" => orca_stl::std_hash(&i32::from_le_bytes(arr(bytes))),
        "i64" => orca_stl::std_hash(&i64::from_le_bytes(arr(bytes))),
        "u64" => orca_stl::std_hash(&u64::from_le_bytes(arr(bytes))),
        "f64" => orca_stl::std_hash(&f64::from_le_bytes(arr(bytes))),
        "f32" => orca_stl::std_hash(&f32::from_le_bytes(arr(bytes))),
        // A std::string of any bytes; a String only for those that are UTF-8.
        "string" => match std::str::from_utf8(bytes) {
            Ok(s) => {
                let h = orca_stl::std_hash(s);
                assert_eq!(
                    h,
                    hash_bytes(bytes),
                    "std_hash of a str is hash_bytes of its bytes"
                );
                h
            }
            Err(_) => hash_bytes(bytes),
        },
        _ => panic!("a HASH record for {ty}"),
    }
}

#[test]
fn std_hash_gives_msvcs_hashes() {
    let file = file();
    let mut m = Mismatches::default();
    for rec in file.of_kind("HASH") {
        let mut r = rec.reader();
        let ty = r.str().unwrap();
        let cases = r.count().unwrap();
        for _ in 0..cases {
            let bytes = r.bytes().unwrap();
            let want = r.u64().unwrap();
            let got = hash_of(&ty, bytes);
            m.check(got == want, || {
                format!("hash<{ty}>({bytes:02x?}) gave {got:#x}, MSVC {want:#x}")
            });
        }
        r.finish("HASH").unwrap();
    }
    m.assert_none("HASH");
}

// ---------------------------------------------------------------- UMAP and USET

/// A snapshot: the key and value pairs in iteration order, the bucket count and the size.
type Snapshot = (Vec<i64>, u64, u64);

#[allow(clippy::cast_possible_truncation)]
fn run_umap(ops: &[(u8, i64, i64)]) -> Vec<Snapshot> {
    let mut m: UnorderedMap<i32, i32> = UnorderedMap::new();
    let mut snaps = Vec::new();
    for &(op, a, b) in ops {
        let (ka, vb) = (a as i32, b as i32);
        match op {
            0 => {
                let v = m.get_or_insert_default(ka);
                *v = v.wrapping_add(vb);
            }
            1 | 2 => {
                m.insert(ka, vb);
            }
            3 => {
                m.remove(&ka);
            }
            4 => m.reserve(size(a)),
            5 => m.rehash(size(a)),
            6 => m.clear(),
            7 => *m.get_or_insert_default(ka) = vb,
            8 => {
                // Map copy(m); m = std::move(copy): the move takes the copy whole.
                let copy = m.clone();
                m = copy;
            }
            9 => {
                // Map t; t[1] = 2; t = m; m = std::move(t).
                let mut t: UnorderedMap<i32, i32> = UnorderedMap::new();
                *t.get_or_insert_default(1) = 2;
                t.clone_from(&m);
                m = t;
            }
            #[allow(clippy::cast_precision_loss)]
            10 => m.set_max_load_factor(a as f32 / 1000.0),
            11 => m = UnorderedMap::with_buckets(size(a)),
            12 => {
                let kv = m
                    .iter()
                    .flat_map(|(k, v)| [i64::from(*k), i64::from(*v)])
                    .collect();
                snaps.push((kv, index(m.bucket_count()), index(m.len())));
            }
            _ => panic!("a UMAP op {op}"),
        }
    }
    snaps
}

#[test]
fn unordered_map_iterates_and_grows_as_msvcs() {
    let file = file();
    let mut m = Mismatches::default();
    for rec in file.of_kind("UMAP") {
        let mut r = rec.reader();
        let name = r.str().unwrap();
        let n = r.count().unwrap();
        let ops: Vec<(u8, i64, i64)> = (0..n)
            .map(|_| (r.u8().unwrap(), r.i64().unwrap(), r.i64().unwrap()))
            .collect();
        let snaps = r.count().unwrap();
        let want: Vec<Snapshot> = (0..snaps)
            .map(|_| {
                (
                    r.vec(Reader::i64).unwrap(),
                    r.u64().unwrap(),
                    r.u64().unwrap(),
                )
            })
            .collect();
        r.finish("UMAP").unwrap();
        let got = run_umap(&ops);
        m.check(got == want, || {
            let at = got.iter().zip(&want).position(|(a, b)| a != b);
            let shown = at.map(|i| (got[i].1, got[i].2, want[i].1, want[i].2));
            format!(
                "{name}: {} snapshots (MSVC {}), first differing {at:?} (buckets, size; MSVC's) {shown:?}",
                got.len(),
                want.len()
            )
        });
    }
    m.assert_none("UMAP");
}

/// Runs a USET script on a set of `K`, keys from their bytes; the iteration order as bytes.
fn run_uset<K, F, G>(ops: &[(u8, Vec<u8>)], from: F, to: G) -> (Vec<Vec<u8>>, u64, u64)
where
    K: orca_stl::StdHashable + PartialEq,
    F: Fn(&[u8]) -> K,
    G: Fn(&K) -> Vec<u8>,
{
    let mut s: UnorderedSet<K> = UnorderedSet::new();
    for (op, key) in ops {
        if *op == 0 {
            s.insert(from(key));
        } else {
            s.remove(&from(key));
        }
    }
    (
        s.iter().map(&to).collect(),
        index(s.bucket_count()),
        index(s.len()),
    )
}

#[test]
fn unordered_sets_iterate_as_msvcs() {
    let file = file();
    let mut m = Mismatches::default();
    for rec in file.of_kind("USET") {
        let mut r = rec.reader();
        let ty = r.str().unwrap();
        let n = r.count().unwrap();
        let ops: Vec<(u8, Vec<u8>)> = (0..n)
            .map(|_| (r.u8().unwrap(), r.bytes().unwrap().to_vec()))
            .collect();
        let count = r.count().unwrap();
        let order: Vec<Vec<u8>> = (0..count).map(|_| r.bytes().unwrap().to_vec()).collect();
        let want = (order, r.u64().unwrap(), r.u64().unwrap());
        r.finish("USET").unwrap();
        let got = match ty.as_str() {
            "string" => run_uset(
                &ops,
                |b| String::from_utf8(b.to_vec()).expect("the probe's strings are ASCII"),
                |s: &String| s.as_bytes().to_vec(),
            ),
            "f64" => run_uset(
                &ops,
                |b| f64::from_le_bytes(b.try_into().unwrap()),
                |x: &f64| x.to_le_bytes().to_vec(),
            ),
            "u64" => run_uset(
                &ops,
                |b| u64::from_le_bytes(b.try_into().unwrap()),
                |x: &u64| x.to_le_bytes().to_vec(),
            ),
            _ => panic!("a USET record of {ty}"),
        };
        m.check(got == want, || {
            format!(
                "unordered_set<{ty}> of {n} ops: buckets {} size {} (MSVC {} {}), same order {}",
                got.1,
                got.2,
                want.1,
                want.2,
                got.0 == want.0
            )
        });
    }
    m.assert_none("USET");
}

// ---------------------------------------------------------------- RNGS, SSEQ and SHUF

/// `mt19937` or `mt19937_64` behind one face.
enum Engine {
    Mt32(Mt19937),
    Mt64(Mt19937_64),
}

#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
fn draw<G: Urbg>(g: &mut G, kind: u8, a: u64, b: u64, discard: impl FnOnce(&mut G, u64)) -> u64 {
    match kind {
        0 => g.next_u64(),
        1 => {
            discard(g, a);
            0
        }
        2 => signed(i64::from(
            UniformIntDistribution::new(a as i32, b as i32).sample(g),
        )),
        3 => signed(UniformIntDistribution::new(a as i64, b as i64).sample(g)),
        4 => UniformIntDistribution::new(a, b).sample(g),
        5 => index(UniformIntDistribution::new(a as usize, b as usize).sample(g)),
        6 => signed(i64::from(
            UniformIntDistribution::new(a as i16, b as i16).sample(g),
        )),
        7 => u64::from(UniformIntDistribution::new(a as u32, b as u32).sample(g)),
        8 => UniformRealDistribution::new(f64::from_bits(a), f64::from_bits(b))
            .sample(g)
            .to_bits(),
        9 => f32_bits(UniformRealDistribution::new(f32_of(a), f32_of(b)).sample(g)),
        10 => orca_stl::generate_canonical_f64(g, 53).to_bits(),
        11 => f32_bits(orca_stl::generate_canonical_f32(g, 24)),
        12 => UniformRealDistribution::<f64>::default()
            .sample(g)
            .to_bits(),
        _ => panic!("an RNGS draw of kind {kind}"),
    }
}

impl Engine {
    fn new(name: &str, seed: &[u64]) -> Engine {
        let seq = || SeedSeq::new(seed[1..].iter().map(|&w| u32::try_from(w).unwrap()));
        #[allow(clippy::cast_possible_truncation)]
        match (name, seed) {
            ("mt19937", []) => Engine::Mt32(Mt19937::new(Mt19937::DEFAULT_SEED)),
            ("mt19937", [s]) => Engine::Mt32(Mt19937::new(*s as u32)),
            ("mt19937_64", []) => Engine::Mt64(Mt19937_64::new(Mt19937_64::DEFAULT_SEED)),
            ("mt19937_64", [s]) => Engine::Mt64(Mt19937_64::new(*s)),
            ("mt19937-seed_seq", _) => Engine::Mt32(Mt19937::from_seed_seq(&seq())),
            ("mt19937_64-seed_seq", _) => Engine::Mt64(Mt19937_64::from_seed_seq(&seq())),
            _ => panic!("an RNGS engine {name} with seed {seed:?}"),
        }
    }

    fn draw(&mut self, kind: u8, a: u64, b: u64) -> u64 {
        match self {
            Engine::Mt32(g) => draw(g, kind, a, b, |g, n| g.discard(n)),
            Engine::Mt64(g) => draw(g, kind, a, b, |g, n| g.discard(n)),
        }
    }
}

#[test]
fn engines_and_distributions_draw_msvcs_values() {
    let file = file();
    let mut m = Mismatches::default();
    for rec in file.of_kind("RNGS") {
        let mut r = rec.reader();
        let name = r.str().unwrap();
        let seed = r.vec(Reader::u64).unwrap();
        let draws = r.count().unwrap();
        let mut engine = Engine::new(&name, &seed);
        for i in 0..draws {
            let (kind, a, b, want) = (
                r.u8().unwrap(),
                r.u64().unwrap(),
                r.u64().unwrap(),
                r.u64().unwrap(),
            );
            let got = engine.draw(kind, a, b);
            m.check(got == want, || {
                format!("{name} seed {seed:?} draw {i} kind {kind} ({a:#x}, {b:#x}) gave {got:#x}, MSVC {want:#x}")
            });
        }
        r.finish("RNGS").unwrap();
    }
    m.assert_none("RNGS");
}

#[test]
fn seed_seq_generates_msvcs_words() {
    let file = file();
    let mut m = Mismatches::default();
    for rec in file.of_kind("SSEQ") {
        let mut r = rec.reader();
        let words = r.vec(Reader::u32).unwrap();
        let want = r.vec(Reader::u32).unwrap();
        r.finish("SSEQ").unwrap();
        let mut got = vec![0u32; want.len()];
        SeedSeq::new(words.iter().copied()).generate(&mut got);
        m.check(got == want, || {
            format!("seed_seq {words:#x?} generated {got:#x?}, MSVC {want:#x?}")
        });
    }
    m.assert_none("SSEQ");
}

#[test]
fn shuffle_gives_msvcs_order() {
    let file = file();
    let mut m = Mismatches::default();
    for rec in file.of_kind("SHUF") {
        let mut r = rec.reader();
        let name = r.str().unwrap();
        let seed = r.u64().unwrap();
        let n = r.u32().unwrap();
        let want = (r.vec(Reader::i32).unwrap(), r.u64().unwrap());
        r.finish("SHUF").unwrap();
        let mut v: Vec<i32> = (0..i32::try_from(n).unwrap()).collect();
        #[allow(clippy::cast_possible_truncation)]
        let next = match name.as_str() {
            "mt19937" => {
                let mut g = Mt19937::new(seed as u32);
                orca_stl::shuffle(&mut v, &mut g);
                u64::from(g.next())
            }
            "mt19937_64" => {
                let mut g = Mt19937_64::new(seed);
                orca_stl::shuffle(&mut v, &mut g);
                g.next()
            }
            _ => panic!("a SHUF record for {name}"),
        };
        let got = (v, next);
        m.check(got == want, || {
            format!(
                "shuffle of {n} with {name}({seed}): same order {}, next {:#x} (MSVC {:#x})",
                got.0 == want.0,
                got.1,
                want.1
            )
        });
    }
    m.assert_none("SHUF");
}
