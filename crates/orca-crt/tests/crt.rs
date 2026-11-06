// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! orca-crt against the static UCRT that this test program links, the C library of the C++
//! engine's `/MT` build, and against what MSVC 14.42's own code gave:
//!
//! - every maths wrapper gives the bits of the UCRT function called through a function pointer
//!   that `black_box` hides from the compiler, so that neither side can be folded or replaced, on
//!   special and seeded random arguments, with the UCRT's FMA3 code paths off and on;
//! - `sqrt`, `sqrtf`, `fabs`, `fabsf` and `copysignf` are the IEEE operations MSVC compiles them
//!   to (`sqrtsd`, `sqrtss` and the sign-bit operations), and for a negative argument, where MSVC
//!   calls the UCRT's `sqrt` instead, the UCRT's NaN is the instruction's;
//! - the float-to-integer conversions equal `cvttsd2si`, `cvttss2si` and MSVC's unsigned 64-bit
//!   sequence, run here as those instructions, on seeded random values, and the values a program
//!   built with MSVC 14.42 (`/O2`, x64) printed for the edge cases;
//! - `rand` and `srand` equal the UCRT's on fresh threads, and the values that program printed.
//!
//! Every case runs with a seeded generator, and each test asserts its count of cases.

use std::arch::asm;
use std::fmt::Debug;
use std::hint::black_box;
use std::sync::Mutex;

/// The UCRT functions the wrappers call, declared again here so that the tests can take their
/// addresses.
mod ucrt {
    unsafe extern "C" {
        pub safe fn sin(x: f64) -> f64;
        pub safe fn cos(x: f64) -> f64;
        pub safe fn tan(x: f64) -> f64;
        pub safe fn asin(x: f64) -> f64;
        pub safe fn acos(x: f64) -> f64;
        pub safe fn atan(x: f64) -> f64;
        pub safe fn atan2(y: f64, x: f64) -> f64;
        pub safe fn sinh(x: f64) -> f64;
        pub safe fn cosh(x: f64) -> f64;
        pub safe fn tanh(x: f64) -> f64;
        pub safe fn asinh(x: f64) -> f64;
        pub safe fn acosh(x: f64) -> f64;
        pub safe fn atanh(x: f64) -> f64;
        pub safe fn exp(x: f64) -> f64;
        pub safe fn exp2(x: f64) -> f64;
        pub safe fn expm1(x: f64) -> f64;
        pub safe fn log(x: f64) -> f64;
        pub safe fn log2(x: f64) -> f64;
        pub safe fn log10(x: f64) -> f64;
        pub safe fn log1p(x: f64) -> f64;
        pub safe fn logb(x: f64) -> f64;
        pub safe fn ilogb(x: f64) -> i32;
        pub safe fn pow(x: f64, y: f64) -> f64;
        pub safe fn fmod(x: f64, y: f64) -> f64;
        pub safe fn remainder(x: f64, y: f64) -> f64;
        pub unsafe fn remquo(x: f64, y: f64, quo: *mut i32) -> f64;
        pub safe fn hypot(x: f64, y: f64) -> f64;
        pub safe fn cbrt(x: f64) -> f64;
        pub safe fn floor(x: f64) -> f64;
        pub safe fn ceil(x: f64) -> f64;
        pub safe fn trunc(x: f64) -> f64;
        pub safe fn round(x: f64) -> f64;
        pub safe fn lround(x: f64) -> i32;
        pub safe fn llround(x: f64) -> i64;
        pub safe fn nearbyint(x: f64) -> f64;
        pub safe fn rint(x: f64) -> f64;
        pub safe fn lrint(x: f64) -> i32;
        pub safe fn llrint(x: f64) -> i64;
        pub unsafe fn modf(x: f64, ip: *mut f64) -> f64;
        pub unsafe fn frexp(x: f64, exp: *mut i32) -> f64;
        pub safe fn ldexp(x: f64, exp: i32) -> f64;
        pub safe fn scalbn(x: f64, exp: i32) -> f64;
        pub safe fn copysign(x: f64, y: f64) -> f64;
        pub safe fn nextafter(x: f64, y: f64) -> f64;
        pub safe fn fma(x: f64, y: f64, z: f64) -> f64;
        pub safe fn fmin(x: f64, y: f64) -> f64;
        pub safe fn fmax(x: f64, y: f64) -> f64;
        pub safe fn fdim(x: f64, y: f64) -> f64;
        pub safe fn erf(x: f64) -> f64;
        pub safe fn erfc(x: f64) -> f64;
        pub safe fn tgamma(x: f64) -> f64;
        pub safe fn lgamma(x: f64) -> f64;
        pub safe fn sqrt(x: f64) -> f64;

        pub safe fn sinf(x: f32) -> f32;
        pub safe fn cosf(x: f32) -> f32;
        pub safe fn tanf(x: f32) -> f32;
        pub safe fn asinf(x: f32) -> f32;
        pub safe fn acosf(x: f32) -> f32;
        pub safe fn atanf(x: f32) -> f32;
        pub safe fn atan2f(y: f32, x: f32) -> f32;
        pub safe fn sinhf(x: f32) -> f32;
        pub safe fn coshf(x: f32) -> f32;
        pub safe fn tanhf(x: f32) -> f32;
        pub safe fn asinhf(x: f32) -> f32;
        pub safe fn acoshf(x: f32) -> f32;
        pub safe fn atanhf(x: f32) -> f32;
        pub safe fn expf(x: f32) -> f32;
        pub safe fn exp2f(x: f32) -> f32;
        pub safe fn expm1f(x: f32) -> f32;
        pub safe fn logf(x: f32) -> f32;
        pub safe fn log2f(x: f32) -> f32;
        pub safe fn log10f(x: f32) -> f32;
        pub safe fn log1pf(x: f32) -> f32;
        pub safe fn powf(x: f32, y: f32) -> f32;
        pub safe fn fmodf(x: f32, y: f32) -> f32;
        pub safe fn remainderf(x: f32, y: f32) -> f32;
        pub safe fn _hypotf(x: f32, y: f32) -> f32;
        pub safe fn cbrtf(x: f32) -> f32;
        pub safe fn floorf(x: f32) -> f32;
        pub safe fn ceilf(x: f32) -> f32;
        pub safe fn truncf(x: f32) -> f32;
        pub safe fn roundf(x: f32) -> f32;
        pub safe fn lroundf(x: f32) -> i32;
        pub safe fn llroundf(x: f32) -> i64;
        pub safe fn nearbyintf(x: f32) -> f32;
        pub safe fn rintf(x: f32) -> f32;
        pub safe fn lrintf(x: f32) -> i32;
        pub safe fn llrintf(x: f32) -> i64;
        pub unsafe fn modff(x: f32, ip: *mut f32) -> f32;
        pub safe fn fmaf(x: f32, y: f32, z: f32) -> f32;
        pub safe fn fminf(x: f32, y: f32) -> f32;
        pub safe fn fmaxf(x: f32, y: f32) -> f32;
        pub safe fn sqrtf(x: f32) -> f32;

        #[link_name = "_set_FMA3_enable"]
        pub safe fn set_fma3_enable(flag: i32) -> i32;
        #[link_name = "_get_FMA3_enable"]
        pub safe fn get_fma3_enable() -> i32;

        pub safe fn rand() -> i32;
        pub safe fn srand(seed: u32);
    }
}

/// `_set_FMA3_enable` is process-wide: the tests that call maths functions hold this lock, so a
/// change of the setting never falls between the two calls of a comparison.
static MATHS: Mutex<()> = Mutex::new(());

/// SplitMix64: a small seeded generator, so every run tests the same cases.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// A value in `[low, high)`, from 53 random bits.
    fn uniform(&mut self, low: f64, high: f64) -> f64 {
        let unit = (self.next() >> 11) as f64 / (1u64 << 53) as f64;
        low + unit * (high - low)
    }
}

/// The bits of a result, for an exact comparison that sees NaN payloads and the sign of zero (a
/// pair's two parts side by side).
trait Bits: Copy + Debug {
    fn bits(self) -> u128;
}

impl Bits for f64 {
    fn bits(self) -> u128 {
        u128::from(self.to_bits())
    }
}

impl Bits for f32 {
    fn bits(self) -> u128 {
        u128::from(self.to_bits())
    }
}

impl Bits for i32 {
    fn bits(self) -> u128 {
        u128::from(self as u32)
    }
}

impl Bits for i64 {
    fn bits(self) -> u128 {
        u128::from(self as u64)
    }
}

impl<A: Bits, B: Bits> Bits for (A, B) {
    fn bits(self) -> u128 {
        (self.0.bits() << 64) | self.1.bits()
    }
}

/// Asserts that two results have the same bits.
fn same<R: Bits>(name: &str, args: &dyn Debug, ours: R, theirs: R) {
    assert!(
        ours.bits() == theirs.bits(),
        "{name}{args:?}: orca-crt {ours:?}, UCRT {theirs:?}"
    );
}

fn check_1<A: Copy + Debug, R: Bits>(
    name: &str,
    ours: fn(A) -> R,
    theirs: extern "C" fn(A) -> R,
    args: &[A],
) -> u64 {
    let theirs = black_box(theirs);
    for &a in args {
        same(name, &(a,), ours(black_box(a)), theirs(black_box(a)));
    }
    args.len() as u64
}

fn check_2<A: Copy + Debug, B: Copy + Debug, R: Bits>(
    name: &str,
    ours: fn(A, B) -> R,
    theirs: extern "C" fn(A, B) -> R,
    args: &[(A, B)],
) -> u64 {
    let theirs = black_box(theirs);
    for &(a, b) in args {
        same(
            name,
            &(a, b),
            ours(black_box(a), black_box(b)),
            theirs(black_box(a), black_box(b)),
        );
    }
    args.len() as u64
}

fn check_3<T: Copy + Debug, R: Bits>(
    name: &str,
    ours: fn(T, T, T) -> R,
    theirs: extern "C" fn(T, T, T) -> R,
    args: &[(T, T, T)],
) -> u64 {
    let theirs = black_box(theirs);
    for &(a, b, c) in args {
        let (x, y) = (black_box((a, b, c)), black_box((a, b, c)));
        same(name, &(a, b, c), ours(x.0, x.1, x.2), theirs(y.0, y.1, y.2));
    }
    args.len() as u64
}

/// Doubles a maths function finds hard, then seeded random ones: any bit pattern, moderate and
/// small magnitudes, values near multiples of pi/2 (argument reduction), near the overflow and
/// underflow of exp, halves and integers (rounding).
fn doubles(rng: &mut Rng) -> Vec<f64> {
    let pi = std::f64::consts::PI;
    let mut v = vec![
        0.0,
        f64::INFINITY,
        f64::from_bits(0x7FF8_0000_0000_0000),
        f64::from_bits(0x7FF8_0000_0000_0001),
        f64::from_bits(0x7FF4_0000_0000_0000),
        f64::from_bits(0x0000_0000_0000_0001),
        f64::from_bits(0x000F_FFFF_FFFF_FFFF),
        f64::MIN_POSITIVE,
        f64::MAX,
        f64::EPSILON,
        1.0,
        0.5,
        1.5,
        2.5,
        2.0,
        3.0,
        0.1,
        pi,
        pi / 2.0,
        pi / 4.0,
        1e-300,
        1e300,
        1e22,
        709.78,
        710.0,
        -745.13,
        -746.0,
        4_503_599_627_370_495.5,
        9_007_199_254_740_991.0,
        9_223_372_036_854_775_808.0,
        2_147_483_648.0,
        2_147_483_647.5,
        1e10,
        171.62,
    ];
    let negative: Vec<f64> = v.iter().map(|x| -x).collect();
    v.extend(negative);
    for i in 0..3000 {
        v.push(match i % 6 {
            0 => f64::from_bits(rng.next()),
            1 => rng.uniform(-10.0, 10.0),
            2 => rng.uniform(-1e6, 1e6),
            3 => orca_crt::ldexp(rng.uniform(-1.0, 1.0), -(rng.below(1000) as i32)),
            4 => (rng.below(2_000_000) as f64 - 1e6) * pi / 2.0 + rng.uniform(-1e-9, 1e-9),
            _ => (rng.below(20_000) as f64 - 1e4) / 2.0,
        });
    }
    v
}

fn floats(rng: &mut Rng) -> Vec<f32> {
    let mut v: Vec<f32> = vec![
        0.0,
        f32::INFINITY,
        f32::from_bits(0x7FC0_0000),
        f32::from_bits(0x7FC0_0001),
        f32::from_bits(0x7FA0_0000),
        f32::from_bits(1),
        f32::from_bits(0x007F_FFFF),
        f32::MIN_POSITIVE,
        f32::MAX,
        f32::EPSILON,
        1.0,
        0.5,
        1.5,
        2.5,
        2.0,
        0.1,
        std::f32::consts::PI,
        std::f32::consts::FRAC_PI_2,
        88.72,
        89.0,
        -103.97,
        -104.0,
        8_388_607.5,
        16_777_215.0,
        2_147_483_648.0,
        35.04,
    ];
    let negative: Vec<f32> = v.iter().map(|x| -x).collect();
    v.extend(negative);
    for i in 0..3000 {
        v.push(match i % 5 {
            0 => f32::from_bits(rng.next() as u32),
            1 => rng.uniform(-10.0, 10.0) as f32,
            2 => rng.uniform(-1e6, 1e6) as f32,
            3 => orca_crt::ldexp(rng.uniform(-1.0, 1.0), -(rng.below(150) as i32)) as f32,
            _ => ((rng.below(20_000) as f64 - 1e4) / 2.0) as f32,
        });
    }
    v
}

/// Pairs of arguments from `xs`: each with a random partner.
fn pairs<T: Copy>(rng: &mut Rng, xs: &[T]) -> Vec<(T, T)> {
    xs.iter()
        .map(|&x| (x, xs[rng.below(xs.len() as u64) as usize]))
        .collect()
}

fn triples<T: Copy>(rng: &mut Rng, xs: &[T]) -> Vec<(T, T, T)> {
    let n = xs.len() as u64;
    xs.iter()
        .map(|&x| (x, xs[rng.below(n) as usize], xs[rng.below(n) as usize]))
        .collect()
}

/// Exponents for `ldexp` and `scalbn`, and integer powers for `pow_int`.
fn exponents(rng: &mut Rng, xs: &[f64]) -> Vec<(f64, i32)> {
    xs.iter()
        .map(|&x| {
            let e = match rng.below(4) {
                0 => [i32::MIN, i32::MAX, 0, 1, -1, 1024, -1075, 2098][rng.below(8) as usize],
                1 => rng.below(4400) as i32 - 2200,
                _ => rng.below(64) as i32 - 32,
            };
            (x, e)
        })
        .collect()
}

/// Every wrapper of `orca_crt::math` against the UCRT function, under the current FMA3 setting.
fn compare_every_wrapper(rng: &mut Rng) -> u64 {
    let d = doubles(rng);
    let f = floats(rng);
    let dd = pairs(rng, &d);
    let ff = pairs(rng, &f);
    let ddd = triples(rng, &d);
    let fff = triples(rng, &f);
    let di = exponents(rng, &d);
    let fi: Vec<(f32, i32)> = exponents(rng, &d)
        .iter()
        .map(|&(x, e)| (x as f32, e))
        .collect();
    let mut n = 0;

    macro_rules! unary {
        ($args:expr; $($name:ident),+) => {
            $(n += check_1(stringify!($name), orca_crt::$name, ucrt::$name, &$args);)+
        };
    }
    macro_rules! binary {
        ($args:expr; $($name:ident),+) => {
            $(n += check_2(stringify!($name), orca_crt::$name, ucrt::$name, &$args);)+
        };
    }

    unary!(d; sin, cos, tan, asin, acos, atan, sinh, cosh, tanh, asinh, acosh, atanh);
    unary!(d; exp, exp2, expm1, log, log2, log10, log1p, logb, ilogb, cbrt);
    unary!(d; floor, ceil, trunc, round, lround, llround, nearbyint, rint, lrint, llrint);
    unary!(d; erf, erfc, tgamma, lgamma);
    binary!(dd; atan2, pow, fmod, remainder, hypot, copysign, nextafter, fmin, fmax, fdim);
    binary!(di; ldexp, scalbn);
    n += check_3("fma", orca_crt::fma, ucrt::fma, &ddd);

    unary!(f; sinf, cosf, tanf, asinf, acosf, atanf, sinhf, coshf, tanhf, asinhf, acoshf, atanhf);
    unary!(f; expf, exp2f, expm1f, logf, log2f, log10f, log1pf, cbrtf);
    unary!(f; floorf, ceilf, truncf, roundf, lroundf, llroundf, nearbyintf, rintf, lrintf, llrintf);
    binary!(ff; atan2f, powf, fmodf, remainderf, fminf, fmaxf);
    n += check_2("hypotf", orca_crt::hypotf, ucrt::_hypotf, &ff);
    n += check_3("fmaf", orca_crt::fmaf, ucrt::fmaf, &fff);

    // The functions with an out-parameter, and the ones the UCRT's header builds from another.
    let remquo: unsafe extern "C" fn(f64, f64, *mut i32) -> f64 = black_box(ucrt::remquo);
    let modf: unsafe extern "C" fn(f64, *mut f64) -> f64 = black_box(ucrt::modf);
    let modff: unsafe extern "C" fn(f32, *mut f32) -> f32 = black_box(ucrt::modff);
    let frexp: unsafe extern "C" fn(f64, *mut i32) -> f64 = black_box(ucrt::frexp);
    let ldexp: extern "C" fn(f64, i32) -> f64 = black_box(ucrt::ldexp);
    let pow: extern "C" fn(f64, f64) -> f64 = black_box(ucrt::pow);
    for &(x, y) in &dd {
        let mut quo = 0;
        // SAFETY: quo is a writable i32 for the length of the call.
        let r = unsafe { remquo(x, y, &raw mut quo) };
        same(
            "remquo",
            &(x, y),
            orca_crt::remquo(black_box(x), black_box(y)),
            (r, quo),
        );
    }
    for &x in &d {
        let mut ip = 0.0;
        // SAFETY: ip is a writable f64 for the length of the call.
        let r = unsafe { modf(x, &raw mut ip) };
        same("modf", &(x,), orca_crt::modf(black_box(x)), (r, ip));
        let mut e = 0;
        // SAFETY: e is a writable i32 for the length of the call.
        let r = unsafe { frexp(x, &raw mut e) };
        same("frexp", &(x,), orca_crt::frexp(black_box(x)), (r, e));
    }
    for &x in &f {
        let mut ip = 0.0;
        // SAFETY: ip is a writable f32 for the length of the call.
        let r = unsafe { modff(x, &raw mut ip) };
        same("modff", &(x,), orca_crt::modff(black_box(x)), (r, ip));
        let mut e = 0;
        // SAFETY: as for frexp above: the header's frexpf is frexp of the widened value.
        let r = unsafe { frexp(f64::from(x), &raw mut e) };
        same(
            "frexpf",
            &(x,),
            orca_crt::frexpf(black_box(x)),
            (r as f32, e),
        );
    }
    for &(x, e) in &fi {
        same(
            "ldexpf",
            &(x, e),
            orca_crt::ldexpf(black_box(x), black_box(e)),
            ldexp(f64::from(x), e) as f32,
        );
    }
    for &(x, e) in &di {
        same(
            "pow_int",
            &(x, e),
            orca_crt::pow_int(black_box(x), black_box(e)),
            pow(x, f64::from(e)),
        );
    }
    n + (dd.len() + 2 * d.len() + 2 * f.len() + fi.len() + di.len()) as u64
}

#[test]
fn every_maths_wrapper_is_the_ucrt_function() {
    let _lock = MATHS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let before = orca_crt::get_fma3_enable();
    let mut cases = 0;
    for fma3 in [0, 1] {
        orca_crt::set_fma3_enable(fma3);
        let mut rng = Rng(0xC127_0001);
        cases += compare_every_wrapper(&mut rng);
    }
    orca_crt::set_fma3_enable(before);
    // 54 comparisons on each of 3068 doubles (or pairs, triples or exponents built on them) and
    // 40 on each of 3052 floats, under each setting.
    assert_eq!(cases, 2 * (54 * 3068 + 40 * 3052));
}

#[test]
fn set_fma3_enable_passes_the_call_through() {
    let _lock = MATHS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let set: extern "C" fn(i32) -> i32 = black_box(ucrt::set_fma3_enable);
    let get: extern "C" fn() -> i32 = black_box(ucrt::get_fma3_enable);
    let before = get();
    assert_eq!(orca_crt::get_fma3_enable(), before);
    assert_eq!(orca_crt::set_fma3_enable(0), set(0));
    assert_eq!((orca_crt::get_fma3_enable(), get()), (0, 0));
    let on = orca_crt::set_fma3_enable(1);
    assert_eq!(on, set(1));
    assert_eq!(orca_crt::get_fma3_enable(), get());
    set(before);
    assert_eq!(orca_crt::get_fma3_enable(), before);
}

/// `sqrtsd x, x`.
fn sqrtsd(mut x: f64) -> f64 {
    // SAFETY: one instruction on a register.
    unsafe { asm!("sqrtsd {0}, {0}", inout(xmm_reg) x, options(pure, nomem, nostack)) };
    x
}

/// `sqrtss x, x`.
fn sqrtss(mut x: f32) -> f32 {
    // SAFETY: one instruction on a register.
    unsafe { asm!("sqrtss {0}, {0}", inout(xmm_reg) x, options(pure, nomem, nostack)) };
    x
}

#[test]
fn sqrt_fabs_and_copysignf_are_ieee_operations() {
    let _lock = MATHS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut rng = Rng(0xC127_0002);
    let d = doubles(&mut rng);
    let f = floats(&mut rng);
    let ucrt_sqrt: extern "C" fn(f64) -> f64 = black_box(ucrt::sqrt);
    let ucrt_sqrtf: extern "C" fn(f32) -> f32 = black_box(ucrt::sqrtf);
    let mut negative = 0;
    for &x in &d {
        let ours = orca_crt::sqrt(black_box(x));
        same("sqrt", &(x,), ours, sqrtsd(x));
        // MSVC calls the UCRT's sqrt for x < 0 (for errno): its NaN is the instruction's.
        if x < 0.0 {
            same("sqrt", &(x,), ours, ucrt_sqrt(x));
            negative += 1;
        }
        same(
            "fabs",
            &(x,),
            orca_crt::fabs(black_box(x)),
            f64::from_bits(x.to_bits() & !(1 << 63)),
        );
    }
    for &x in &f {
        let ours = orca_crt::sqrtf(black_box(x));
        same("sqrtf", &(x,), ours, sqrtss(x));
        if x < 0.0 {
            same("sqrtf", &(x,), ours, ucrt_sqrtf(x));
            negative += 1;
        }
        same(
            "fabsf",
            &(x,),
            orca_crt::fabsf(black_box(x)),
            f32::from_bits(x.to_bits() & !(1 << 31)),
        );
    }
    for (x, y) in pairs(&mut rng, &f) {
        let bits = (x.to_bits() & !(1 << 31)) | (y.to_bits() & (1 << 31));
        same(
            "copysignf",
            &(x, y),
            orca_crt::copysignf(black_box(x), black_box(y)),
            f32::from_bits(bits),
        );
    }
    assert_eq!(d.len() + f.len(), 3068 + 3052);
    assert!(negative > 2000, "{negative} negative arguments");
}

/// `cvttsd2si r32`.
fn cvttsd2si_32(x: f64) -> i32 {
    let r: i32;
    // SAFETY: one instruction on registers.
    unsafe {
        asm!("cvttsd2si {0:e}, {1}", out(reg) r, in(xmm_reg) x, options(pure, nomem, nostack))
    };
    r
}

/// `cvttsd2si r64`.
fn cvttsd2si_64(x: f64) -> i64 {
    let r: i64;
    // SAFETY: one instruction on registers.
    unsafe { asm!("cvttsd2si {0}, {1}", out(reg) r, in(xmm_reg) x, options(pure, nomem, nostack)) };
    r
}

/// `cvttss2si r32`.
fn cvttss2si_32(x: f32) -> i32 {
    let r: i32;
    // SAFETY: one instruction on registers.
    unsafe {
        asm!("cvttss2si {0:e}, {1}", out(reg) r, in(xmm_reg) x, options(pure, nomem, nostack))
    };
    r
}

/// `cvttss2si r64`.
fn cvttss2si_64(x: f32) -> i64 {
    let r: i64;
    // SAFETY: one instruction on registers.
    unsafe { asm!("cvttss2si {0}, {1}", out(reg) r, in(xmm_reg) x, options(pure, nomem, nostack)) };
    r
}

/// MSVC 14.42's `(uint64_t)x` for a `double`, the instructions it emits.
fn msvc_f64_to_u64(x: f64) -> u64 {
    let r: u64;
    // SAFETY: arithmetic on registers, no memory and no stack.
    unsafe {
        asm!(
            "xor {c:e}, {c:e}",
            "comisd {x}, {two63}",
            "jb 2f",
            "subsd {x}, {two63}",
            "comisd {x}, {two63}",
            "jae 2f",
            "mov {c}, {sign}",
            "2:",
            "cvttsd2si {r}, {x}",
            "add {r}, {c}",
            x = inout(xmm_reg) x => _,
            two63 = in(xmm_reg) 9_223_372_036_854_775_808.0f64,
            sign = in(reg) 0x8000_0000_0000_0000u64,
            c = out(reg) _,
            r = out(reg) r,
            options(pure, nomem, nostack),
        );
    }
    r
}

/// MSVC 14.42's `(uint64_t)x` for a `float`: the same with `comiss`, `subss` and `cvttss2si`.
fn msvc_f32_to_u64(x: f32) -> u64 {
    let r: u64;
    // SAFETY: arithmetic on registers, no memory and no stack.
    unsafe {
        asm!(
            "xor {c:e}, {c:e}",
            "comiss {x}, {two63}",
            "jb 2f",
            "subss {x}, {two63}",
            "comiss {x}, {two63}",
            "jae 2f",
            "mov {c}, {sign}",
            "2:",
            "cvttss2si {r}, {x}",
            "add {r}, {c}",
            x = inout(xmm_reg) x => _,
            two63 = in(xmm_reg) 9_223_372_036_854_775_808.0f32,
            sign = in(reg) 0x8000_0000_0000_0000u64,
            c = out(reg) _,
            r = out(reg) r,
            options(pure, nomem, nostack),
        );
    }
    r
}

/// An argument's bits and its casts to the eight integer types, narrowest first.
type Casts<T> = (T, i8, u8, i16, u16, i32, u32, i64, u64);

/// What a program built with MSVC 14.42 (`cl /O2 /EHsc /MT`, x64) printed for these casts of a
/// `double`, each in an out-of-line function: the bits, then `(int8_t)`, `(uint8_t)`, `(int16_t)`,
/// `(uint16_t)`, `(int32_t)`, `(uint32_t)`, `(int64_t)` and `(uint64_t)`.
#[rustfmt::skip]
const MSVC_DOUBLE_CASTS: [Casts<u64>; 56] = [
    (0x0000_0000_0000_0000, 0, 0, 0, 0, 0, 0, 0, 0),
    (0x8000_0000_0000_0000, 0, 0, 0, 0, 0, 0, 0, 0),
    (0x3FE0_0000_0000_0000, 0, 0, 0, 0, 0, 0, 0, 0),
    (0xBFE0_0000_0000_0000, 0, 0, 0, 0, 0, 0, 0, 0),
    (0x3FEF_F7CE_D916_872B, 0, 0, 0, 0, 0, 0, 0, 0),
    (0xBFEF_F7CE_D916_872B, 0, 0, 0, 0, 0, 0, 0, 0),
    (0x3FF0_0000_0000_0000, 1, 1, 1, 1, 1, 1, 1, 1),
    (0xBFF0_0000_0000_0000, -1, 255, -1, 65535, -1, 4294967295, -1, 18446744073709551615),
    (0x405F_C000_0000_0000, 127, 127, 127, 127, 127, 127, 127, 127),
    (0x405F_F999_9999_999A, 127, 127, 127, 127, 127, 127, 127, 127),
    (0x4060_0000_0000_0000, -128, 128, 128, 128, 128, 128, 128, 128),
    (0xC060_0000_0000_0000, -128, 128, -128, 65408, -128, 4294967168, -128, 18446744073709551488),
    (0xC060_1CCC_CCCC_CCCD, -128, 128, -128, 65408, -128, 4294967168, -128, 18446744073709551488),
    (0xC060_2000_0000_0000, 127, 127, -129, 65407, -129, 4294967167, -129, 18446744073709551487),
    (0x406F_E000_0000_0000, -1, 255, 255, 255, 255, 255, 255, 255),
    (0x406F_FCCC_CCCC_CCCD, -1, 255, 255, 255, 255, 255, 255, 255),
    (0x4070_0000_0000_0000, 0, 0, 256, 256, 256, 256, 256, 256),
    (0x40DF_FFC0_0000_0000, -1, 255, 32767, 32767, 32767, 32767, 32767, 32767),
    (0x40E0_0000_0000_0000, 0, 0, -32768, 32768, 32768, 32768, 32768, 32768),
    (0xC0E0_0000_0000_0000, 0, 0, -32768, 32768, -32768, 4294934528, -32768, 18446744073709518848),
    (0xC0E0_0020_0000_0000, -1, 255, 32767, 32767, -32769, 4294934527, -32769, 18446744073709518847),
    (0x40EF_FFE0_0000_0000, -1, 255, -1, 65535, 65535, 65535, 65535, 65535),
    (0x40F0_0000_0000_0000, 0, 0, 0, 0, 65536, 65536, 65536, 65536),
    (0x40F1_1700_0000_0000, 112, 112, 4464, 4464, 70000, 70000, 70000, 70000),
    (0xC0F1_1700_0000_0000, -112, 144, -4464, 61072, -70000, 4294897296, -70000, 18446744073709481616),
    (0x41DF_FFFF_FFC0_0000, -1, 255, -1, 65535, 2147483647, 2147483647, 2147483647, 2147483647),
    (0x41DF_FFFF_FFE0_0000, -1, 255, -1, 65535, 2147483647, 2147483647, 2147483647, 2147483647),
    (0x41E0_0000_0000_0000, 0, 0, 0, 0, -2147483648, 2147483648, 2147483648, 2147483648),
    (0xC1E0_0000_0000_0000, 0, 0, 0, 0, -2147483648, 2147483648, -2147483648, 18446744071562067968),
    (0xC1E0_0000_0010_0000, 0, 0, 0, 0, -2147483648, 2147483648, -2147483648, 18446744071562067968),
    (0xC1E0_0000_0020_0000, 0, 0, 0, 0, -2147483648, 2147483647, -2147483649, 18446744071562067967),
    (0x41EF_FFFF_FFE0_0000, 0, 0, 0, 0, -2147483648, 4294967295, 4294967295, 4294967295),
    (0x41EF_FFFF_FFF0_0000, 0, 0, 0, 0, -2147483648, 4294967295, 4294967295, 4294967295),
    (0x41F0_0000_0000_0000, 0, 0, 0, 0, -2147483648, 0, 4294967296, 4294967296),
    (0x4202_A05F_2000_0000, 0, 0, 0, 0, -2147483648, 1410065408, 10000000000, 10000000000),
    (0xC202_A05F_2000_0000, 0, 0, 0, 0, -2147483648, 2884901888, -10000000000, 18446744063709551616),
    (0x43DF_FFFF_FFFF_FFFF, 0, 0, 0, 0, -2147483648, 4294966272, 9223372036854774784, 9223372036854774784),
    (0x43E0_0000_0000_0000, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0xC3E0_0000_0000_0000, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0xC3E0_0000_0000_0001, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0x43EF_FFFF_FFFF_FFFF, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 18446744073709549568),
    (0x43F0_0000_0000_0000, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0x4400_0000_0000_0000, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0x43E1_58E4_6091_3D00, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 10000000000000000000),
    (0x4415_AF1D_78B5_8C40, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0x4629_3E59_39A0_8CEA, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0xC629_3E59_39A0_8CEA, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0x7FE1_CCF3_85EB_C8A0, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0xFFE1_CCF3_85EB_C8A0, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0x0000_0000_0000_0001, 0, 0, 0, 0, 0, 0, 0, 0),
    (0x8000_0000_0000_0001, 0, 0, 0, 0, 0, 0, 0, 0),
    (0x7FF0_0000_0000_0000, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0xFFF0_0000_0000_0000, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0x7FF8_0000_0000_0000, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0xFFF8_0000_0000_0000, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0x7FF0_0000_0000_0001, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
];

/// The same program's casts of a `float`.
#[rustfmt::skip]
const MSVC_FLOAT_CASTS: [Casts<u32>; 49] = [
    (0x0000_0000, 0, 0, 0, 0, 0, 0, 0, 0),
    (0x1FCC_72F3, 0, 0, 0, 0, 0, 0, 0, 0),
    (0x3F7F_BE77, 0, 0, 0, 0, 0, 0, 0, 0),
    (0x437F_0000, -1, 255, 255, 255, 255, 255, 255, 255),
    (0x437F_E666, -1, 255, 255, 255, 255, 255, 255, 255),
    (0x44C5_3965, 41, 41, 1577, 1577, 1577, 1577, 1577, 1577),
    (0x4780_0000, 0, 0, 0, 0, 65536, 65536, 65536, 65536),
    (0x4788_B800, 112, 112, 4464, 4464, 70000, 70000, 70000, 70000),
    (0x49F9_7265, 76, 76, 11852, 11852, 2043468, 2043468, 2043468, 2043468),
    (0x4F00_0000, 0, 0, 0, 0, -2147483648, 2147483648, 2147483648, 2147483648),
    (0x4F80_0000, 0, 0, 0, 0, -2147483648, 0, 4294967296, 4294967296),
    (0x5015_02F9, 0, 0, 0, 0, -2147483648, 1410065408, 10000000000, 10000000000),
    (0x5A04_4460, 0, 0, 0, 0, -2147483648, 0, 9307469008338944, 9307469008338944),
    (0x5F00_0000, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0x5F80_0000, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0x6000_0000, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0x60AD_78EC, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0x7149_F2CA, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0x7493_1017, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0x7B51_84B8, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0x7F80_0000, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0x7FC0_0000, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0x8000_0000, 0, 0, 0, 0, 0, 0, 0, 0),
    (0x832F_6BBD, 0, 0, 0, 0, 0, 0, 0, 0),
    (0x8694_7BBE, 0, 0, 0, 0, 0, 0, 0, 0),
    (0x8B71_65F5, 0, 0, 0, 0, 0, 0, 0, 0),
    (0x9F84_951D, 0, 0, 0, 0, 0, 0, 0, 0),
    (0x9FD9_F0EB, 0, 0, 0, 0, 0, 0, 0, 0),
    (0xA88C_3036, 0, 0, 0, 0, 0, 0, 0, 0),
    (0xABCA_E4C4, 0, 0, 0, 0, 0, 0, 0, 0),
    (0xBBAF_1E07, 0, 0, 0, 0, 0, 0, 0, 0),
    (0xBF00_0000, 0, 0, 0, 0, 0, 0, 0, 0),
    (0xBF80_0000, -1, 255, -1, 65535, -1, 4294967295, -1, 18446744073709551615),
    (0xC300_0000, -128, 128, -128, 65408, -128, 4294967168, -128, 18446744073709551488),
    (0xC300_E666, -128, 128, -128, 65408, -128, 4294967168, -128, 18446744073709551488),
    (0xC301_0000, 127, 127, -129, 65407, -129, 4294967167, -129, 18446744073709551487),
    (0xC700_0000, 0, 0, -32768, 32768, -32768, 4294934528, -32768, 18446744073709518848),
    (0xC96F_E176, -23, 233, 489, 489, -982551, 4293984745, -982551, 18446744073708569065),
    (0xCB5E_FD7C, -124, 132, 644, 644, -14613884, 4280353412, -14613884, 18446744073694937732),
    (0xCF00_0000, 0, 0, 0, 0, -2147483648, 2147483648, -2147483648, 18446744071562067968),
    (0xD015_02F9, 0, 0, 0, 0, -2147483648, 2884901888, -10000000000, 18446744063709551616),
    (0xD132_C8A3, 0, 0, 0, 0, -2147483648, 3547713536, -47991894016, 18446744025717657600),
    (0xDCA9_F45E, 0, 0, 0, 0, -2147483648, 0, -382703645025632256, 18064040428683919360),
    (0xDF00_0000, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0xEDF6_062A, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0xF149_F2CA, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0xF504_27AC, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0xF77A_D5DE, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
    (0xFFC0_0000, 0, 0, 0, 0, -2147483648, 0, -9223372036854775808, 9223372036854775808),
];

#[test]
fn conversions_give_what_msvc_gave() {
    for &(bits, i8_, u8_, i16_, u16_, i32_, u32_, i64_, u64_) in &MSVC_DOUBLE_CASTS {
        let x = black_box(f64::from_bits(bits));
        let ours = (
            orca_crt::f64_to_i8(x),
            orca_crt::f64_to_u8(x),
            orca_crt::f64_to_i16(x),
            orca_crt::f64_to_u16(x),
            orca_crt::f64_to_i32(x),
            orca_crt::f64_to_u32(x),
            orca_crt::f64_to_i64(x),
            orca_crt::f64_to_u64(x),
        );
        assert_eq!(
            ours,
            (i8_, u8_, i16_, u16_, i32_, u32_, i64_, u64_),
            "{x:e} ({bits:#018x})"
        );
    }
    for &(bits, i8_, u8_, i16_, u16_, i32_, u32_, i64_, u64_) in &MSVC_FLOAT_CASTS {
        let x = black_box(f32::from_bits(bits));
        let ours = (
            orca_crt::f32_to_i8(x),
            orca_crt::f32_to_u8(x),
            orca_crt::f32_to_i16(x),
            orca_crt::f32_to_u16(x),
            orca_crt::f32_to_i32(x),
            orca_crt::f32_to_u32(x),
            orca_crt::f32_to_i64(x),
            orca_crt::f32_to_u64(x),
        );
        assert_eq!(
            ours,
            (i8_, u8_, i16_, u16_, i32_, u32_, i64_, u64_),
            "{x:e} ({bits:#010x})"
        );
    }
}

#[test]
fn conversions_are_msvc_instructions() {
    let mut rng = Rng(0xC127_0003);
    let mut cases = 0;
    for i in 0..400_000 {
        // Any bit pattern, or a value near a power of two from 2^-1 to 2^65 (the limits of every
        // width and their neighbours).
        let x = if i % 2 == 0 {
            f64::from_bits(rng.next())
        } else {
            let p = rng.below(67) as i32 - 1;
            let near = orca_crt::ldexp(1.0 + rng.uniform(-1e-6, 1e-6), p);
            let near = if rng.below(4) == 0 {
                near.trunc() + rng.below(3) as f64 - 1.0
            } else {
                near
            };
            if rng.below(2) == 0 { -near } else { near }
        };
        let x = black_box(x);
        let (r32, r64) = (cvttsd2si_32(x), cvttsd2si_64(x));
        assert_eq!(orca_crt::f64_to_i8(x), r32 as i8, "{x:e}");
        assert_eq!(orca_crt::f64_to_u8(x), r32 as u8, "{x:e}");
        assert_eq!(orca_crt::f64_to_i16(x), r32 as i16, "{x:e}");
        assert_eq!(orca_crt::f64_to_u16(x), r32 as u16, "{x:e}");
        assert_eq!(orca_crt::f64_to_i32(x), r32, "{x:e}");
        assert_eq!(orca_crt::f64_to_u32(x), r64 as u32, "{x:e}");
        assert_eq!(orca_crt::f64_to_i64(x), r64, "{x:e}");
        assert_eq!(orca_crt::f64_to_u64(x), msvc_f64_to_u64(x), "{x:e}");

        let y = black_box(if i % 2 == 0 {
            f32::from_bits(rng.next() as u32)
        } else {
            x as f32
        });
        let (r32, r64) = (cvttss2si_32(y), cvttss2si_64(y));
        assert_eq!(orca_crt::f32_to_i8(y), r32 as i8, "{y:e}");
        assert_eq!(orca_crt::f32_to_u8(y), r32 as u8, "{y:e}");
        assert_eq!(orca_crt::f32_to_i16(y), r32 as i16, "{y:e}");
        assert_eq!(orca_crt::f32_to_u16(y), r32 as u16, "{y:e}");
        assert_eq!(orca_crt::f32_to_i32(y), r32, "{y:e}");
        assert_eq!(orca_crt::f32_to_u32(y), r64 as u32, "{y:e}");
        assert_eq!(orca_crt::f32_to_i64(y), r64, "{y:e}");
        assert_eq!(orca_crt::f32_to_u64(y), msvc_f32_to_u64(y), "{y:e}");
        cases += 16;
    }
    assert_eq!(cases, 6_400_000);
}

/// The first values of `rand()` that the MSVC program printed on a new thread, and after `srand`
/// of these seeds.
#[rustfmt::skip]
const MSVC_RAND: [(Option<u32>, [i32; 12]); 9] = [
    (None, [41, 18467, 6334, 26500, 19169, 15724, 11478, 29358, 26962, 24464, 5705, 28145]),
    (Some(0), [38, 7719, 21238, 2437, 8855, 11797, 8365, 32285, 10450, 30612, 5853, 28100]),
    (Some(1), [41, 18467, 6334, 26500, 19169, 15724, 11478, 29358, 26962, 24464, 5705, 28145]),
    (Some(2), [45, 29216, 24198, 17795, 29484, 19650, 14590, 26431, 10705, 18316, 5557, 28189]),
    (Some(42), [175, 400, 17869, 30056, 16083, 12879, 8016, 7644, 15809, 1769, 32409, 29950]),
    (Some(12345), [7584, 19164, 25795, 22125, 5828, 23405, 27477, 5413, 29072, 23404, 14959, 14800]),
    (Some(2147483647), [35, 29739, 3374, 11141, 31308, 7870, 5253, 2445, 26706, 3992, 6001, 28056]),
    (Some(2147483648), [38, 7719, 21238, 2437, 8855, 11797, 8365, 32285, 10450, 30612, 5853, 28100]),
    (Some(4294967295), [35, 29739, 3374, 11141, 31308, 7870, 5253, 2445, 26706, 3992, 6001, 28056]),
];

/// On a new thread: `srand(seed)` if given, then `count` values of orca-crt's `rand` and of the
/// UCRT's.
fn rand_sequences(seed: Option<u32>, count: usize) -> (Vec<i32>, Vec<i32>) {
    std::thread::spawn(move || {
        let ucrt_rand: extern "C" fn() -> i32 = black_box(ucrt::rand);
        let ucrt_srand: extern "C" fn(u32) = black_box(ucrt::srand);
        if let Some(seed) = seed {
            orca_crt::srand(seed);
            ucrt_srand(seed);
        }
        let ours = (0..count).map(|_| orca_crt::rand()).collect();
        let theirs = (0..count).map(|_| ucrt_rand()).collect();
        (ours, theirs)
    })
    .join()
    .unwrap()
}

#[test]
fn rand_is_the_ucrt_generator_per_thread() {
    let mut rng = Rng(0xC127_0004);
    let mut seeds: Vec<Option<u32>> = MSVC_RAND.iter().map(|&(seed, _)| seed).collect();
    seeds.extend((0..40).map(|_| Some(rng.next() as u32)));
    let mut cases = 0;
    for seed in seeds {
        let (ours, theirs) = rand_sequences(seed, 5000);
        assert_eq!(ours, theirs, "seed {seed:?}");
        assert!(ours.iter().all(|&v| (0..=orca_crt::RAND_MAX).contains(&v)));
        if let Some((_, msvc)) = MSVC_RAND.iter().find(|&&(s, _)| s == seed) {
            assert_eq!(ours[..12], msvc[..], "seed {seed:?}");
        }
        cases += ours.len();
    }
    assert_eq!(cases, 49 * 5000);
}

#[test]
fn rand_state_restarts_a_thread() {
    std::thread::spawn(|| {
        use orca_crt::rand::{RAND_INITIAL_STATE, rand_state, set_rand_state};
        assert_eq!(rand_state(), RAND_INITIAL_STATE);
        let first: Vec<i32> = (0..100).map(|_| orca_crt::rand()).collect();
        orca_crt::srand(7);
        assert_eq!(rand_state(), 7);
        let _ = orca_crt::rand();
        set_rand_state(RAND_INITIAL_STATE);
        let again: Vec<i32> = (0..100).map(|_| orca_crt::rand()).collect();
        assert_eq!(first, again);
        assert_eq!(first[..12], MSVC_RAND[0].1[..]);
    })
    .join()
    .unwrap();
}
