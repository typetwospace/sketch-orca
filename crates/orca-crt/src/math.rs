// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The static UCRT's maths functions, called as the C++ engine calls them.
//!
//! Each public function is one `#[inline(never)]` wrapper around the function of the same name in
//! the `libucrt.lib` the program links statically (`.cargo/config.toml`, `+crt-static`), the
//! library the C++ engine's `/MT` build takes its maths from. Nothing here computes a maths result
//! itself: the UCRT's source is not open, so it is called, never translated. The crate is
//! `#![no_builtins]`, so LLVM does not recognise these calls as library functions it knows: it
//! does not fold them into constants, rewrite `pow(x, 2.0)` into `x * x`, or answer with its own
//! code. `#[inline(never)]` keeps that so in the calling crates too, which are not `no_builtins`:
//! a call there is a call to this wrapper, which calls the UCRT.
//!
//! What MSVC 14.42 (`/O2`, `/fp:precise`, x64, the C++ engine's flags) makes of each C++ call:
//!
//! - `std::sin(double)` and the other `double` functions, `std::ceil`, `std::floor`,
//!   `std::round`, `std::trunc` and `std::copysign` included, call the UCRT function: these wrappers.
//! - `std::sin(float)` and the other `float` overloads call the UCRT's `sinf` and so on (on x64
//!   they are functions of their own): the `*f` wrappers.
//! - `std::frexp(float)` and `std::ldexp(float)` are `frexpf` and `ldexpf`, which the UCRT's header
//!   defines inline as the `double` function and a conversion; `std::hypot(float)` is the header's
//!   `hypotf`, which calls `_hypotf`. [`frexpf`], [`ldexpf`] and [`hypotf`] do the same.
//! - `std::pow(double, int)` and `std::pow(float, int)` are promoted to `pow(double, double)` (the
//!   `float` one returns a `double`): [`pow_int`].
//! - `std::copysign(float)` becomes bit operations, `std::fabs` an `andps` and `std::sqrt` a
//!   `sqrtsd` (`sqrtss`): IEEE operations, so [`copysignf`], [`fabs`], [`fabsf`], [`sqrt`] and
//!   [`sqrtf`] are Rust's own, which compile to the same instructions. For a negative argument
//!   MSVC calls the UCRT's `sqrt` instead of `sqrtsd` (for `errno`); its NaN is the instruction's,
//!   which `tests/crt.rs` checks.
//!
//! The UCRT picks code paths that use FMA3 instructions when the processor has them, unless
//! `_set_FMA3_enable(0)` turned them off: [`set_fma3_enable`] and [`get_fma3_enable`] pass the call
//! through, for the probes that record the C++ engine's maths both ways.

/// The functions of the static UCRT (`corecrt_math.h`, x64), as `libucrt.lib` exports them.
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

        #[link_name = "_set_FMA3_enable"]
        pub safe fn set_fma3_enable(flag: i32) -> i32;
        #[link_name = "_get_FMA3_enable"]
        pub safe fn get_fma3_enable() -> i32;
    }
}

/// One wrapper per UCRT function that takes and returns floating-point values by value.
macro_rules! wrap {
    ($($name:ident ( $($arg:ident: $t:ty),+ ) -> $r:ty;)+) => {
        $(
            #[doc = concat!("`", stringify!($name), "` of the static UCRT.")]
            #[inline(never)]
            #[must_use]
            pub fn $name($($arg: $t),+) -> $r {
                ucrt::$name($($arg),+)
            }
        )+
    };
}

wrap! {
    sin(x: f64) -> f64;
    cos(x: f64) -> f64;
    tan(x: f64) -> f64;
    asin(x: f64) -> f64;
    acos(x: f64) -> f64;
    atan(x: f64) -> f64;
    atan2(y: f64, x: f64) -> f64;
    sinh(x: f64) -> f64;
    cosh(x: f64) -> f64;
    tanh(x: f64) -> f64;
    asinh(x: f64) -> f64;
    acosh(x: f64) -> f64;
    atanh(x: f64) -> f64;
    exp(x: f64) -> f64;
    exp2(x: f64) -> f64;
    expm1(x: f64) -> f64;
    log(x: f64) -> f64;
    log2(x: f64) -> f64;
    log10(x: f64) -> f64;
    log1p(x: f64) -> f64;
    logb(x: f64) -> f64;
    ilogb(x: f64) -> i32;
    pow(x: f64, y: f64) -> f64;
    fmod(x: f64, y: f64) -> f64;
    remainder(x: f64, y: f64) -> f64;
    hypot(x: f64, y: f64) -> f64;
    cbrt(x: f64) -> f64;
    floor(x: f64) -> f64;
    ceil(x: f64) -> f64;
    trunc(x: f64) -> f64;
    round(x: f64) -> f64;
    lround(x: f64) -> i32;
    llround(x: f64) -> i64;
    nearbyint(x: f64) -> f64;
    rint(x: f64) -> f64;
    lrint(x: f64) -> i32;
    llrint(x: f64) -> i64;
    ldexp(x: f64, exp: i32) -> f64;
    scalbn(x: f64, exp: i32) -> f64;
    copysign(x: f64, y: f64) -> f64;
    nextafter(x: f64, y: f64) -> f64;
    fma(x: f64, y: f64, z: f64) -> f64;
    fmin(x: f64, y: f64) -> f64;
    fmax(x: f64, y: f64) -> f64;
    fdim(x: f64, y: f64) -> f64;
    erf(x: f64) -> f64;
    erfc(x: f64) -> f64;
    tgamma(x: f64) -> f64;
    lgamma(x: f64) -> f64;

    sinf(x: f32) -> f32;
    cosf(x: f32) -> f32;
    tanf(x: f32) -> f32;
    asinf(x: f32) -> f32;
    acosf(x: f32) -> f32;
    atanf(x: f32) -> f32;
    atan2f(y: f32, x: f32) -> f32;
    sinhf(x: f32) -> f32;
    coshf(x: f32) -> f32;
    tanhf(x: f32) -> f32;
    asinhf(x: f32) -> f32;
    acoshf(x: f32) -> f32;
    atanhf(x: f32) -> f32;
    expf(x: f32) -> f32;
    exp2f(x: f32) -> f32;
    expm1f(x: f32) -> f32;
    logf(x: f32) -> f32;
    log2f(x: f32) -> f32;
    log10f(x: f32) -> f32;
    log1pf(x: f32) -> f32;
    powf(x: f32, y: f32) -> f32;
    fmodf(x: f32, y: f32) -> f32;
    remainderf(x: f32, y: f32) -> f32;
    cbrtf(x: f32) -> f32;
    floorf(x: f32) -> f32;
    ceilf(x: f32) -> f32;
    truncf(x: f32) -> f32;
    roundf(x: f32) -> f32;
    lroundf(x: f32) -> i32;
    llroundf(x: f32) -> i64;
    nearbyintf(x: f32) -> f32;
    rintf(x: f32) -> f32;
    lrintf(x: f32) -> i32;
    llrintf(x: f32) -> i64;
    fmaf(x: f32, y: f32, z: f32) -> f32;
    fminf(x: f32, y: f32) -> f32;
    fmaxf(x: f32, y: f32) -> f32;
}

/// `remquo` of the static UCRT: the remainder and the low bits of the quotient, with its sign.
#[inline(never)]
#[must_use]
pub fn remquo(x: f64, y: f64) -> (f64, i32) {
    let mut quo = 0;
    // SAFETY: `quo` is a valid, writable i32 for the length of the call.
    let r = unsafe { ucrt::remquo(x, y, &raw mut quo) };
    (r, quo)
}

/// `modf` of the static UCRT: the fractional part and the integral part, in that order.
#[inline(never)]
#[must_use]
pub fn modf(x: f64) -> (f64, f64) {
    let mut ip = 0.0;
    // SAFETY: `ip` is a valid, writable f64 for the length of the call.
    let frac = unsafe { ucrt::modf(x, &raw mut ip) };
    (frac, ip)
}

/// `modff` of the static UCRT: the fractional part and the integral part, in that order.
#[inline(never)]
#[must_use]
pub fn modff(x: f32) -> (f32, f32) {
    let mut ip = 0.0;
    // SAFETY: `ip` is a valid, writable f32 for the length of the call.
    let frac = unsafe { ucrt::modff(x, &raw mut ip) };
    (frac, ip)
}

/// `frexp` of the static UCRT: the mantissa and the exponent, in that order.
#[inline(never)]
#[must_use]
pub fn frexp(x: f64) -> (f64, i32) {
    let mut exp = 0;
    // SAFETY: `exp` is a valid, writable i32 for the length of the call.
    let m = unsafe { ucrt::frexp(x, &raw mut exp) };
    (m, exp)
}

/// `frexpf` as the UCRT's header defines it: `(float)frexp((double)x, &exp)`.
#[inline(never)]
#[must_use]
pub fn frexpf(x: f32) -> (f32, i32) {
    let mut exp = 0;
    // SAFETY: `exp` is a valid, writable i32 for the length of the call.
    let m = unsafe { ucrt::frexp(f64::from(x), &raw mut exp) };
    // The narrowing the header's cast does: exact, since the mantissa of a float has 24 bits.
    (m as f32, exp)
}

/// `ldexpf` as the UCRT's header defines it: `(float)ldexp((double)x, exp)`.
#[inline(never)]
#[must_use]
pub fn ldexpf(x: f32, exp: i32) -> f32 {
    // The header's cast, cvtsd2ss: rounds to nearest, as Rust's `as` does.
    ucrt::ldexp(f64::from(x), exp) as f32
}

/// `hypotf` as the UCRT's header defines it: a call to `_hypotf`.
#[inline(never)]
#[must_use]
pub fn hypotf(x: f32, y: f32) -> f32 {
    ucrt::_hypotf(x, y)
}

/// `std::pow(double, int)` and `std::pow(float, int)`: C++ promotes both arguments to `double`
/// and calls `pow(double, double)` (MSVC: `cvtdq2pd` and a call to `pow`). The `float` form
/// returns this `double` too; C++ code that stores it in a `float` narrows it with `as f32`.
#[inline(never)]
#[must_use]
pub fn pow_int(x: f64, n: i32) -> f64 {
    ucrt::pow(x, f64::from(n))
}

/// `sqrt`: MSVC compiles `std::sqrt(double)` to `sqrtsd`, an IEEE operation, and calls the UCRT's
/// `sqrt` only for a negative argument, which gives the same NaN as the instruction
/// (`tests/crt.rs`).
#[inline]
#[must_use]
pub fn sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// `sqrtf`: `sqrtss`, as [`sqrt`] is `sqrtsd`.
#[inline]
#[must_use]
pub fn sqrtf(x: f32) -> f32 {
    x.sqrt()
}

/// `fabs`: MSVC compiles it to `andps` with the sign mask, as Rust's `abs` is compiled.
#[inline]
#[must_use]
pub fn fabs(x: f64) -> f64 {
    x.abs()
}

/// `fabsf`: `andps` with the sign mask.
#[inline]
#[must_use]
pub fn fabsf(x: f32) -> f32 {
    x.abs()
}

/// `std::copysign(float)`: MSVC inlines it as bit operations (`xorps`, `andps`, `xorps`), the
/// IEEE `copySign` that Rust's `copysign` is.
#[inline]
#[must_use]
pub fn copysignf(x: f32, y: f32) -> f32 {
    x.copysign(y)
}

/// `_set_FMA3_enable` of the static UCRT: 0 turns off the FMA3 code paths of its maths functions,
/// 1 lets the UCRT use them where the processor has FMA3. Returns what the UCRT returns. For the
/// probes and tests that record results both ways; the engine never calls it.
#[inline(never)]
pub fn set_fma3_enable(flag: i32) -> i32 {
    ucrt::set_fma3_enable(flag)
}

/// `_get_FMA3_enable` of the static UCRT: whether its maths functions may use FMA3 (1) or not (0).
#[inline(never)]
#[must_use]
pub fn get_fma3_enable() -> i32 {
    ucrt::get_fma3_enable()
}
