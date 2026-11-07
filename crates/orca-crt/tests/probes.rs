// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-crt: every record of `crt.sop1` (`parity/SOP1.md`, "Probe files"),
//! which the probe program wrote with the C++ engine's own compile and link lines, replayed
//! against the crate, bit for bit.
//!
//! - MATH: each maths function the oracle's linker map lists, with the UCRT's FMA3 code paths on
//!   and off, on every recorded argument: the results' bits.
//! - MATS: `nan(tag)`'s bits for every tag.
//! - CAST: each `double` and `float` to integer conversion, on every recorded value.
//! - RAND: `rand` on a fresh thread, after `srand` on a fresh thread, and per thread.
//! - QSRT: `qsort` on keys with ties, with Qhull's three kinds of comparator and elements of 4, 8
//!   and 16 bytes: every comparator call in order (the two elements and their places in the
//!   array, and the result), and the order it left.
//!
//! The committed subset (`parity/probes/`) runs by default; `SKETCH_ORCA_PROBES_DEEP=<folder>`
//! replays the full set from that folder (`<work>/parity/probes`).

use std::sync::Mutex;

use orca_crt as crt;
use orca_probe::{Mismatches, ProbeFile, Reader, load_probe};

/// The UCRT's FMA3 choice is the process's: one test at a time changes it.
static FMA3: Mutex<()> = Mutex::new(());

fn file() -> ProbeFile {
    load_probe("crt").unwrap_or_else(|e| panic!("{e}"))
}

fn d(x: u64) -> f64 {
    f64::from_bits(x)
}

#[allow(clippy::cast_possible_truncation)]
fn f(x: u64) -> f32 {
    f32::from_bits(x as u32)
}

fn fd(x: f64) -> u64 {
    x.to_bits()
}

fn ff(x: f32) -> u64 {
    u64::from(x.to_bits())
}

#[allow(clippy::cast_sign_loss)]
fn int(x: i64) -> u64 {
    x as u64
}

#[allow(clippy::cast_possible_truncation)]
fn as_i32(x: u64) -> i32 {
    x as i64 as i32
}

/// A maths function as one shape: the arguments' bits in, the results' bits out.
type MathFn = Box<dyn Fn(&[u64]) -> Vec<u64>>;

/// The crate's function for a MATH record's name and signature.
fn math_fn(name: &str, sig: &str) -> Option<MathFn> {
    macro_rules! d1 {
        ($f:path) => {
            Some(Box::new(|a: &[u64]| vec![fd($f(d(a[0])))]))
        };
    }
    macro_rules! d2 {
        ($f:path) => {
            Some(Box::new(|a: &[u64]| vec![fd($f(d(a[0]), d(a[1])))]))
        };
    }
    macro_rules! f1 {
        ($f:path) => {
            Some(Box::new(|a: &[u64]| vec![ff($f(f(a[0])))]))
        };
    }
    macro_rules! f2 {
        ($f:path) => {
            Some(Box::new(|a: &[u64]| vec![ff($f(f(a[0]), f(a[1])))]))
        };
    }
    macro_rules! di {
        ($f:path) => {
            Some(Box::new(|a: &[u64]| vec![int(i64::from($f(d(a[0]))))]))
        };
    }
    let out: Option<MathFn> = match (name, sig) {
        ("acos", "d>d") => d1!(crt::acos),
        ("asin", "d>d") => d1!(crt::asin),
        ("atan", "d>d") => d1!(crt::atan),
        ("ceil", "d>d") => d1!(crt::ceil),
        ("cos", "d>d") => d1!(crt::cos),
        ("exp", "d>d") => d1!(crt::exp),
        ("fabs", "d>d") => d1!(crt::fabs),
        ("floor", "d>d") => d1!(crt::floor),
        ("log", "d>d") => d1!(crt::log),
        ("log10", "d>d") => d1!(crt::log10),
        ("nearbyint", "d>d") => d1!(crt::nearbyint),
        ("round", "d>d") => d1!(crt::round),
        ("sin", "d>d") => d1!(crt::sin),
        ("sqrt", "d>d") => d1!(crt::sqrt),
        ("tan", "d>d") => d1!(crt::tan),
        ("trunc", "d>d") => d1!(crt::trunc),
        ("atan2", "dd>d") => d2!(crt::atan2),
        ("copysign", "dd>d") => d2!(crt::copysign),
        ("_copysign", "dd>d") => d2!(crt::ms::copysign),
        ("fmax", "dd>d") => d2!(crt::fmax),
        ("fmin", "dd>d") => d2!(crt::fmin),
        ("fmod", "dd>d") => d2!(crt::fmod),
        ("hypot", "dd>d") => d2!(crt::hypot),
        ("_hypot", "dd>d") => d2!(crt::ms::hypot),
        ("nextafter", "dd>d") => d2!(crt::nextafter),
        ("nexttoward", "dd>d") => d2!(crt::nexttoward),
        ("pow", "dd>d") => d2!(crt::pow),
        ("acosf", "f>f") => f1!(crt::acosf),
        ("atanf", "f>f") => f1!(crt::atanf),
        ("ceilf", "f>f") => f1!(crt::ceilf),
        ("cosf", "f>f") => f1!(crt::cosf),
        ("expf", "f>f") => f1!(crt::expf),
        ("floorf", "f>f") => f1!(crt::floorf),
        ("logf", "f>f") => f1!(crt::logf),
        ("roundf", "f>f") => f1!(crt::roundf),
        ("sinf", "f>f") => f1!(crt::sinf),
        ("sqrtf", "f>f") => f1!(crt::sqrtf),
        ("tanf", "f>f") => f1!(crt::tanf),
        ("truncf", "f>f") => f1!(crt::truncf),
        ("asinf", "f>f") => f1!(crt::asinf),
        ("atan2f", "ff>f") => f2!(crt::atan2f),
        ("fmaxf", "ff>f") => f2!(crt::fmaxf),
        ("fminf", "ff>f") => f2!(crt::fminf),
        ("fmodf", "ff>f") => f2!(crt::fmodf),
        ("powf", "ff>f") => f2!(crt::powf),
        ("fma", "ddd>d") => Some(Box::new(|a: &[u64]| {
            vec![fd(crt::fma(d(a[0]), d(a[1]), d(a[2])))]
        })),
        ("llround", "d>l") => Some(Box::new(|a: &[u64]| vec![int(crt::llround(d(a[0])))])),
        ("llrint", "d>l") => Some(Box::new(|a: &[u64]| vec![int(crt::llrint(d(a[0])))])),
        ("lround", "d>i") => di!(crt::lround),
        ("lrint", "d>i") => di!(crt::lrint),
        ("ilogb", "d>i") => di!(crt::ilogb),
        ("_finite", "d>i") => di!(crt::ms::finite),
        ("_fpclass", "d>i") => di!(crt::ms::fpclass),
        ("_isnan", "d>i") => di!(crt::ms::isnan),
        ("frexp", "d>di") => Some(Box::new(|a: &[u64]| {
            let (m, e) = crt::frexp(d(a[0]));
            vec![fd(m), int(i64::from(e))]
        })),
        ("ldexp", "di>d") => Some(Box::new(|a: &[u64]| {
            vec![fd(crt::ldexp(d(a[0]), as_i32(a[1])))]
        })),
        ("__vdecl_floor2", "dd>dd") => Some(Box::new(|a: &[u64]| {
            let (x, y) = crt::floor2(d(a[0]), d(a[1]));
            vec![fd(x), fd(y)]
        })),
        _ => None,
    };
    out
}

/// Arguments and results per case of a MATH signature.
fn arity(sig: &str) -> (usize, usize) {
    match sig {
        "d>d" | "f>f" | "d>l" | "d>i" => (1, 1),
        "dd>d" | "ff>f" | "di>d" => (2, 1),
        "ddd>d" => (3, 1),
        "d>di" => (1, 2),
        "dd>dd" => (2, 2),
        _ => panic!("a MATH record of signature {sig}, which this test does not know"),
    }
}

#[test]
fn math_functions_give_the_ucrt_bits_with_fma3_on_and_off() {
    let _guard = FMA3
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = file();
    let before = crt::get_fma3_enable();
    let mut m = Mismatches::default();
    let mut functions = 0;
    for rec in file.of_kind("MATH") {
        let mut r = rec.reader();
        let name = r.str().unwrap();
        let fma3 = r.i32().unwrap();
        let sig = r.str().unwrap();
        let cases = r.count().unwrap();
        let (na, nr) = arity(&sig);
        let Some(call) = math_fn(&name, &sig) else {
            m.check(false, || {
                format!("MATH {name} ({sig}): orca-crt has no such function")
            });
            continue;
        };
        functions += 1;
        crt::set_fma3_enable(fma3);
        for i in 0..cases {
            let args: Vec<u64> = (0..na).map(|_| r.u64().unwrap()).collect();
            let want: Vec<u64> = (0..nr).map(|_| r.u64().unwrap()).collect();
            let got = call(&args);
            m.check(got == want, || {
                format!("MATH {name} FMA3 {fma3} case {i}: args {args:#018x?} gave {got:#018x?}, the UCRT {want:#018x?}")
            });
        }
        r.finish("MATH").unwrap();
    }
    crt::set_fma3_enable(before);
    assert!(functions >= 100, "only {functions} MATH records");
    m.assert_none("MATH");
}

#[test]
fn nan_tags_give_the_ucrt_bits() {
    let file = file();
    let mut m = Mismatches::default();
    for rec in file.of_kind("MATS") {
        let mut r = rec.reader();
        assert_eq!(r.str().unwrap(), "nan");
        let _fma3 = r.i32().unwrap();
        let cases = r.count().unwrap();
        for _ in 0..cases {
            let tag = r.str().unwrap();
            let want = r.u64().unwrap();
            let c = std::ffi::CString::new(tag.clone()).unwrap();
            let got = crt::nan(&c).to_bits();
            m.check(got == want, || {
                format!("nan({tag:?}) gave {got:#018x}, the UCRT {want:#018x}")
            });
        }
        r.finish("MATS").unwrap();
    }
    m.assert_none("MATS");
}

#[allow(clippy::cast_sign_loss)]
fn cast(from: &str, to: &str, x: u64) -> u64 {
    macro_rules! go {
        ($d:path, $f:path, signed) => {
            if from == "f64" {
                i64::from($d(d(x))) as u64
            } else {
                i64::from($f(f(x))) as u64
            }
        };
        ($d:path, $f:path, unsigned) => {
            if from == "f64" {
                u64::from($d(d(x)))
            } else {
                u64::from($f(f(x)))
            }
        };
    }
    match to {
        "i8" => go!(crt::f64_to_i8, crt::f32_to_i8, signed),
        "u8" => go!(crt::f64_to_u8, crt::f32_to_u8, unsigned),
        "i16" => go!(crt::f64_to_i16, crt::f32_to_i16, signed),
        "u16" => go!(crt::f64_to_u16, crt::f32_to_u16, unsigned),
        "i32" => go!(crt::f64_to_i32, crt::f32_to_i32, signed),
        "u32" => go!(crt::f64_to_u32, crt::f32_to_u32, unsigned),
        "i64" => go!(crt::f64_to_i64, crt::f32_to_i64, signed),
        "u64" => go!(crt::f64_to_u64, crt::f32_to_u64, unsigned),
        _ => panic!("a CAST to {to}"),
    }
}

#[test]
fn casts_give_msvcs_integers() {
    let file = file();
    let mut m = Mismatches::default();
    let mut records = 0;
    for rec in file.of_kind("CAST") {
        records += 1;
        let mut r = rec.reader();
        let what = r.str().unwrap();
        let (from, to) = what.split_once('>').unwrap();
        let cases = r.count().unwrap();
        for _ in 0..cases {
            let x = r.u64().unwrap();
            let want = r.u64().unwrap();
            let got = cast(from, to, x);
            m.check(got == want, || {
                format!("CAST {what} of {x:#018x} gave {got:#x}, MSVC {want:#x}")
            });
        }
        r.finish("CAST").unwrap();
    }
    assert_eq!(
        records, 16,
        "the CAST records: f64 and f32 to eight integer types"
    );
    m.assert_none("CAST");
}

fn read_rand(r: &mut Reader<'_>) -> (String, u32, Vec<i32>) {
    let what = r.str().unwrap();
    let seed = r.u32().unwrap();
    let values = r.vec(Reader::i32).unwrap();
    (what, seed, values)
}

#[test]
fn rand_is_the_ucrts_per_thread() {
    let file = file();
    let mut m = Mismatches::default();
    for rec in file.of_kind("RAND") {
        let mut r = rec.reader();
        let (what, seed, want) = read_rand(&mut r);
        r.finish("RAND").unwrap();
        let n = want.len();
        let got: Vec<i32> = match what.as_str() {
            "fresh-thread" => std::thread::spawn(move || (0..n).map(|_| crt::rand()).collect())
                .join()
                .unwrap(),
            "srand" => std::thread::spawn(move || {
                crt::srand(seed);
                (0..n).map(|_| crt::rand()).collect()
            })
            .join()
            .unwrap(),
            "per-thread" => std::thread::spawn(move || {
                let mut v = vec![crt::rand()];
                std::thread::spawn(move || crt::srand(seed)).join().unwrap();
                v.extend((1..n).map(|_| crt::rand()));
                v
            })
            .join()
            .unwrap(),
            _ => panic!("a RAND record of {what}"),
        };
        m.check(got == want, || {
            format!("RAND {what} {seed}: {got:?}, the UCRT {want:?}")
        });
    }
    m.assert_none("RAND");
}

/// One comparator call of a QSRT record: the two elements' ids, their places in the array (65535
/// for a pointer that is not at an element) and the result's low 16 bits.
type QsortCall = (u16, u16, u16, u16, u16);

/// The place of a pointer that is not at an element of the array.
const QSORT_OUTSIDE: u16 = 0xFFFF;

/// An element of a QSRT record's array, laid out as the probe program's (`parity/SOP1.md`, QSRT).
trait QsortElem: Copy {
    fn new(key: i32, id: u32) -> Self;
    fn key(&self) -> i32;
    fn id(&self) -> u32;
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Q4(u32);

#[derive(Clone, Copy)]
#[repr(C)]
struct Q8 {
    key: i32,
    id: u32,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Q16 {
    key: i32,
    id: u32,
    pad: u64,
}

impl QsortElem for Q4 {
    #[allow(clippy::cast_sign_loss)]
    fn new(key: i32, id: u32) -> Self {
        Q4(((key as u32) << 16) | id)
    }
    #[allow(clippy::cast_possible_wrap)]
    fn key(&self) -> i32 {
        (self.0 >> 16) as i32
    }
    fn id(&self) -> u32 {
        self.0 & 0xFFFF
    }
}

impl QsortElem for Q8 {
    fn new(key: i32, id: u32) -> Self {
        Q8 { key, id }
    }
    fn key(&self) -> i32 {
        self.key
    }
    fn id(&self) -> u32 {
        self.id
    }
}

impl QsortElem for Q16 {
    fn new(key: i32, id: u32) -> Self {
        Q16 {
            key,
            id,
            pad: u64::from(id).wrapping_mul(0x9E37_79B9_7F4A_7C15),
        }
    }
    fn key(&self) -> i32 {
        self.key
    }
    fn id(&self) -> u32 {
        self.id
    }
}

/// The C comparators the probe program sorts with, Qhull's three kinds.
fn qsort_compare_keys(kind: u32, a: i32, b: i32) -> i32 {
    match kind {
        0 => a - b,
        1 => {
            if a > b {
                1
            } else {
                -1
            }
        }
        _ => i32::from(a > b) - i32::from(a < b),
    }
}

/// Sorts `keys` as elements `E` with orca-crt's `qsort`: the comparator calls and the ids left.
#[allow(clippy::cast_possible_truncation)]
fn qsort_replay<E: QsortElem>(kind: u32, keys: &[i32]) -> (Vec<QsortCall>, Vec<u32>) {
    let mut v: Vec<E> = keys.iter().zip(0..).map(|(&k, id)| E::new(k, id)).collect();
    let (base, n, size) = (v.as_ptr() as usize, v.len(), size_of::<E>());
    let place = |p: *const E| {
        let p = p as usize;
        if p < base || !(p - base).is_multiple_of(size) || (p - base) / size >= n {
            QSORT_OUTSIDE
        } else {
            ((p - base) / size) as u16
        }
    };
    let mut calls = Vec::new();
    crt::qsort(&mut v, |a, b| {
        let result = qsort_compare_keys(kind, a.key(), b.key());
        calls.push((
            a.id() as u16,
            b.id() as u16,
            place(std::ptr::from_ref(a)),
            place(std::ptr::from_ref(b)),
            result as u16,
        ));
        result
    });
    (calls, v.iter().map(QsortElem::id).collect())
}

#[test]
fn qsort_makes_the_ucrts_comparator_calls() {
    let file = file();
    let mut m = Mismatches::default();
    for rec in file.of_kind("QSRT") {
        let mut r = rec.reader();
        let kind = r.u32().unwrap();
        let size = r.u32().unwrap();
        let keys = r.vec(Reader::i32).unwrap();
        let want_calls = r
            .vec(|r| Ok((r.u16()?, r.u16()?, r.u16()?, r.u16()?, r.u16()?)))
            .unwrap();
        let want_ids = r.vec(Reader::u32).unwrap();
        r.finish("QSRT").unwrap();
        let got = match size {
            4 => qsort_replay::<Q4>(kind, &keys),
            8 => qsort_replay::<Q8>(kind, &keys),
            16 => qsort_replay::<Q16>(kind, &keys),
            _ => panic!("a QSRT record of {size}-byte elements"),
        };
        m.check(got.0 == want_calls && got.1 == want_ids, || {
            let first = got.0.iter().zip(&want_calls).position(|(g, w)| g != w);
            format!(
                "QSRT comparator {kind}, {size} bytes, keys {keys:?}: {} calls, the UCRT {} \
                 (first difference at {first:?}); ids {:?}, the UCRT {want_ids:?}",
                got.0.len(),
                want_calls.len(),
                got.1
            )
        });
    }
    m.assert_none("QSRT");
}
