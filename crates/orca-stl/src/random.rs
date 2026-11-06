// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Microsoft STL code: this file is translated from stl/inc/random (mersenne_twister and
// mersenne_twister_engine with their seeding, seed_seq, _Generate_canonical_params,
// generate_canonical, _Nrand_impl, _Rng_from_urng_v2, uniform_int, uniform_int_distribution and
// uniform_real) and stl/inc/algorithm (_Rng_from_urng, _Random_shuffle1 and shuffle) of
// microsoft/STL at the tag vs-2022-17.12 (commit 91e425587a0b9b5ac0d1005844c661fee2e2813b), MSVC
// 14.42's random numbers, engine call for engine call, on 2026-11-06. Its copyright holder is
// Microsoft Corporation, and it is under Apache-2.0 WITH LLVM-exception (licenses/Apache-2.0.txt
// and licenses/LLVM-exception.txt).

//! `std::mt19937`, `std::mt19937_64`, `std::seed_seq`, `std::generate_canonical`,
//! `std::uniform_int_distribution`, `std::uniform_real_distribution` and `std::shuffle`, as MSVC
//! computes them.
//!
//! The engines' sequences are fixed by the C++ standard. The distributions and `shuffle` are
//! not: each library turns engine output into numbers its own way, with its own number of engine
//! calls. These follow MSVC's: [`UniformIntDistribution`] is `_Rng_from_urng_v2` (Lemire's
//! multiply-and-reject), [`UniformRealDistribution`] is `generate_canonical * (b - a) + a`, and
//! [`shuffle`] is `_Random_shuffle1` with the older `_Rng_from_urng` (divide-and-reject), so a
//! seeded engine gives the C++ program's numbers and leaves the engine where the C++ program
//! leaves it.

/// A uniform random bit generator whose `min()` is 0 and whose `max()` is `2^BITS - 1`, as
/// `mt19937` (32 bits) and `mt19937_64` (64 bits) are.
pub trait Urbg {
    /// The width of `result_type` and of the range, 32 or 64.
    const BITS: u32;

    /// `operator()`, widened to `u64`.
    fn next_u64(&mut self) -> u64;
}

macro_rules! mersenne_twister {
    (
        $(#[$doc:meta])*
        $name:ident, $t:ty, w = $w:expr, n = $n:expr, m = $m:expr, r = $r:expr, a = $a:expr,
        u = $u:expr, d = $d:expr, s = $s:expr, b = $b:expr, t = $tt:expr, c = $c:expr,
        l = $l:expr, f = $f:expr
    ) => {
        $(#[$doc])*
        #[derive(Clone)]
        pub struct $name {
            x: [$t; $n],
            idx: usize,
        }

        impl $name {
            /// `default_seed`.
            pub const DEFAULT_SEED: $t = 5489;
            const N: usize = $n;
            const M: usize = $m;
            const UPPER: $t = <$t>::MAX << $r;
            const LOWER: $t = !Self::UPPER;

            /// `engine(seed)`.
            pub fn new(seed: $t) -> Self {
                let mut g = $name { x: [0; $n], idx: $n };
                g.seed(seed);
                g
            }

            /// `engine(seq)`: seeded from a seed sequence.
            pub fn from_seed_seq(seq: &SeedSeq) -> Self {
                let mut g = $name { x: [0; $n], idx: $n };
                g.seed_seq(seq);
                g
            }

            /// `seed(value)`: `x[0] = value`, `x[i] = f * (x[i-1] ^ (x[i-1] >> (w-2))) + i`.
            pub fn seed(&mut self, value: $t) {
                let mut prev = value;
                for (i, slot) in self.x.iter_mut().enumerate() {
                    if i > 0 {
                        prev = (i as $t).wrapping_add($f.wrapping_mul(prev ^ (prev >> ($w - 2))));
                    }
                    *slot = prev;
                }
                self.idx = Self::N;
            }

            /// `seed(seq)`: `(w + 31) / 32` generated words per state word; an all-zero state
            /// (the top bit of the first word aside) becomes `x[0] = 2^(w-1)`.
            pub fn seed_seq(&mut self, seq: &SeedSeq) {
                const K: usize = ($w as usize).div_ceil(32);
                let mut arr = vec![0u32; K * $n];
                seq.generate(&mut arr);
                let mut sum: $t = 0;
                for (i, (slot, words)) in self.x.iter_mut().zip(arr.chunks_exact(K)).enumerate() {
                    let mut v: $t = 0;
                    for (j, &word) in words.iter().enumerate() {
                        v |= <$t>::from(word) << (32 * j);
                    }
                    *slot = v;
                    if i == 0 {
                        sum = v >> $r;
                    } else {
                        sum |= v;
                    }
                }
                if sum == 0 {
                    let top: $t = 1;
                    self.x[0] = top << ($w - 1);
                }
                self.idx = Self::N;
            }

            fn twist(&mut self) {
                for i in 0..Self::N {
                    let y = (self.x[i] & Self::UPPER) | (self.x[(i + 1) % Self::N] & Self::LOWER);
                    let mag = if y & 1 != 0 { $a } else { 0 };
                    self.x[i] = self.x[(i + Self::M) % Self::N] ^ (y >> 1) ^ mag;
                }
                self.idx = 0;
            }

            /// `operator()`.
            #[allow(clippy::should_implement_trait)]
            pub fn next(&mut self) -> $t {
                if self.idx >= Self::N {
                    self.twist();
                }
                let mut y = self.x[self.idx];
                self.idx += 1;
                y ^= (y >> $u) & $d;
                y ^= (y << $s) & $b;
                y ^= (y << $tt) & $c;
                y ^= y >> $l;
                y
            }

            /// `discard(n)`: `n` calls.
            pub fn discard(&mut self, n: u64) {
                for _ in 0..n {
                    self.next();
                }
            }
        }

        impl Default for $name {
            /// `engine()`: seeded with 5489.
            fn default() -> Self {
                Self::new(Self::DEFAULT_SEED)
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct(stringify!($name)).field("idx", &self.idx).finish_non_exhaustive()
            }
        }

        impl Urbg for $name {
            const BITS: u32 = $w;

            #[inline]
            fn next_u64(&mut self) -> u64 {
                u64::from(self.next())
            }
        }
    };
}

mersenne_twister!(
    /// `std::mt19937` (and MSVC's `std::default_random_engine`).
    Mt19937, u32, w = 32, n = 624, m = 397, r = 31, a = 0x9908_b0df_u32, u = 11, d = 0xffff_ffff_u32,
    s = 7, b = 0x9d2c_5680_u32, t = 15, c = 0xefc6_0000_u32, l = 18, f = 1_812_433_253_u32
);

mersenne_twister!(
    /// `std::mt19937_64`.
    Mt19937_64, u64, w = 64, n = 312, m = 156, r = 31, a = 0xb502_6f5a_a966_19e9_u64, u = 29,
    d = 0x5555_5555_5555_5555_u64, s = 17, b = 0x71d6_7fff_eda6_0000_u64, t = 37,
    c = 0xfff7_eee0_0000_0000_u64, l = 43, f = 6_364_136_223_846_793_005_u64
);

/// `std::default_random_engine`, which is `mt19937` in MSVC.
pub type DefaultRandomEngine = Mt19937;

/// `std::seed_seq`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SeedSeq {
    v: Vec<u32>,
}

impl SeedSeq {
    /// `seed_seq{values...}`: each value truncated to 32 bits.
    pub fn new<I: IntoIterator<Item = u32>>(values: I) -> Self {
        SeedSeq {
            v: values.into_iter().collect(),
        }
    }

    /// `size()`.
    pub fn len(&self) -> usize {
        self.v.len()
    }

    /// `size() == 0`.
    pub fn is_empty(&self) -> bool {
        self.v.is_empty()
    }

    /// `param(out)`: the stored values.
    pub fn param(&self) -> &[u32] {
        &self.v
    }

    /// `generate(first, last)`.
    pub fn generate(&self, out: &mut [u32]) {
        let n = out.len();
        if n == 0 {
            return;
        }
        let s = self.v.len();
        let t = if 623 <= n {
            11
        } else if 68 <= n {
            7
        } else if 39 <= n {
            5
        } else if 7 <= n {
            3
        } else {
            (n - 1) / 2
        };
        let p = (n - t) / 2;
        let q = p + t;
        let m = if n <= s { s + 1 } else { n };
        let xor27 = |v: u32| v ^ (v >> 27);
        // `(k - 1) % n` is computed in size_t, so k = 0 gives (2^64 - 1) % n.
        let back = |k: usize| k.wrapping_sub(1) % n;

        out.fill(0x8b8b_8b8b);
        let mut k = 0usize;
        while k < m {
            let r1 = 1_664_525u32.wrapping_mul(xor27(out[k % n] ^ out[(k + p) % n] ^ out[back(k)]));
            let off: usize = if k == 0 {
                s
            } else if k <= s {
                k % n + self.v[k - 1] as usize
            } else {
                k % n
            };
            let r2 = (u64::from(r1).wrapping_add(off as u64) & 0xffff_ffff) as u32;
            out[(k + p) % n] = out[(k + p) % n].wrapping_add(r1);
            out[(k + q) % n] = out[(k + q) % n].wrapping_add(r2);
            out[k % n] = r2;
            k += 1;
        }
        while k < m + n {
            let r3 = 1_566_083_941u32.wrapping_mul(xor27(
                out[k % n]
                    .wrapping_add(out[(k + p) % n])
                    .wrapping_add(out[back(k)]),
            ));
            let r4 = r3.wrapping_sub((k % n) as u32);
            out[(k + p) % n] ^= r3;
            out[(k + q) % n] ^= r4;
            out[k % n] = r4;
            k += 1;
        }
    }
}

/// `_Generate_canonical_params` for an engine of `engine_bits` bits and `min_bits` wanted bits:
/// (k, discarded bits).
fn canonical_params(min_bits: u32, engine_bits: u32) -> (u32, u32) {
    let k = min_bits.div_ceil(engine_bits);
    (k, engine_bits * k - min_bits)
}

/// `std::generate_canonical<double, bits>(g)`: a value in `[0, 1)` from `min(53, bits)` engine
/// bits (MSVC's power-of-two path: the high bits of the first call, the next calls above them).
pub fn generate_canonical_f64<G: Urbg + ?Sized>(g: &mut G, bits: u32) -> f64 {
    let min_bits = bits.min(53);
    if min_bits == 0 {
        return 0.0;
    }
    let (k, discarded) = canonical_params(min_bits, G::BITS);
    let mut sx: u64 = g.next_u64() >> discarded;
    let mut shift = -(discarded as i32);
    for _ in 1..k {
        shift += G::BITS as i32;
        sx = sx.wrapping_add(g.next_u64().wrapping_shl(shift as u32));
    }
    let scale = 1.0f32 / (1u64 << min_bits) as f32;
    sx as f64 * f64::from(scale)
}

/// `std::generate_canonical<float, bits>(g)`: a value in `[0, 1)` from `min(24, bits)` engine
/// bits.
pub fn generate_canonical_f32<G: Urbg + ?Sized>(g: &mut G, bits: u32) -> f32 {
    let min_bits = bits.min(24);
    if min_bits == 0 {
        return 0.0;
    }
    let (k, discarded) = canonical_params(min_bits, G::BITS);
    let mut sx: u32 = (g.next_u64() >> discarded) as u32;
    let mut shift = -(discarded as i32);
    for _ in 1..k {
        shift += G::BITS as i32;
        sx = sx.wrapping_add((g.next_u64() as u32).wrapping_shl(shift as u32));
    }
    let scale = 1.0f32 / (1u64 << min_bits) as f32;
    sx as f32 * scale
}

/// An integer type `uniform_int_distribution` takes: `short`, `int`, `long long` and their
/// unsigned forms (`long` is `int` on Windows).
pub trait UniformInt: Copy + PartialOrd {
    /// The width of the type in bits.
    const WIDTH: u32;
    /// Whether it is signed.
    const SIGNED: bool;
    /// The type's smallest value.
    const MIN: Self;
    /// The type's largest value.
    const MAX: Self;
    /// `static_cast<make_unsigned_t<T>>(self)`, zero-extended.
    fn to_unsigned(self) -> u64;
    /// `static_cast<T>(u)` of a value of `WIDTH` bits.
    fn from_unsigned(u: u64) -> Self;
}

macro_rules! uniform_int_type {
    ($($t:ty, $u:ty, $signed:expr);*) => {$(
        impl UniformInt for $t {
            const WIDTH: u32 = <$t>::BITS;
            const SIGNED: bool = $signed;
            const MIN: Self = <$t>::MIN;
            const MAX: Self = <$t>::MAX;
            #[inline]
            fn to_unsigned(self) -> u64 {
                self as $u as u64
            }
            #[inline]
            fn from_unsigned(u: u64) -> Self {
                u as $u as $t
            }
        }
    )*};
}
uniform_int_type!(i16, u16, true; u16, u16, false; i32, u32, true; u32, u32, false;
    i64, u64, true; u64, u64, false; isize, usize, true; usize, usize, false);

fn width_mask(bits: u32) -> u64 {
    if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    }
}

/// `_Rng_from_urng_v2<Uty, Engine>`: `Uty` of `ty_bits` bits, `Udiff` the wider of it and the
/// engine's type.
struct RngV2 {
    ty_bits: u32,
    udiff_bits: u32,
    bits: u32,
    bmask: u128,
}

impl RngV2 {
    fn new<G: Urbg + ?Sized>(ty_bits: u32) -> Self {
        let udiff_bits = crate::max(ty_bits, G::BITS);
        RngV2 {
            ty_bits,
            udiff_bits,
            bits: G::BITS,
            bmask: u128::from(width_mask(G::BITS)),
        }
    }

    fn udiff_mask(&self) -> u128 {
        u128::from(width_mask(self.udiff_bits))
    }

    /// `static_cast<Udiff>(index - 1)`, with `index - 1` in `Uty`'s promoted type.
    fn index_minus_one(&self, index: u64) -> u128 {
        let v = if self.ty_bits < 32 {
            // promoted to int: 0 - 1 is -1, sign-extended into Udiff
            (index as i64 - 1) as u64
        } else {
            index.wrapping_sub(1) & width_mask(self.ty_bits)
        };
        u128::from(v) & self.udiff_mask()
    }

    fn random_product<G: Urbg + ?Sized>(&self, g: &mut G, index: u64, niter: u32) -> u128 {
        let mut ret = u128::from(g.next_u64());
        for _ in 1..niter {
            ret = ((ret << self.bits) | u128::from(g.next_u64())) & self.udiff_mask();
        }
        ret * u128::from(index)
    }

    /// `operator()(index)`: a value in `[0, index)`.
    fn below<G: Urbg + ?Sized>(&self, g: &mut G, index: u64) -> u64 {
        let mut mask = self.bmask;
        let mut niter = 1u32;
        if self.bits < self.udiff_bits {
            let limit = self.index_minus_one(index);
            while mask < limit {
                mask = ((mask << self.bits) | self.bmask) & self.udiff_mask();
                niter += 1;
            }
        }
        let index_w = u128::from(index);
        let mut product = self.random_product(g, index, niter);
        let mut rem = product & mask;
        if rem < index_w {
            let threshold = (mask - index_w + 1) % index_w;
            while rem < threshold {
                product = self.random_product(g, index, niter);
                rem = product & mask;
            }
        }
        let generated_bits = if self.bits < self.udiff_bits {
            mask.count_ones()
        } else {
            self.udiff_bits
        };
        ((product >> generated_bits) as u64) & width_mask(self.ty_bits)
    }

    /// `_Get_all_bits()`.
    fn all_bits<G: Urbg + ?Sized>(&self, g: &mut G) -> u64 {
        let mut ret = u128::from(g.next_u64());
        if self.bits < self.udiff_bits {
            let mut num = self.bits;
            while num < self.udiff_bits {
                ret = ((ret << self.bits) | u128::from(g.next_u64())) & self.udiff_mask();
                num += self.bits;
            }
        }
        ret as u64
    }
}

/// `std::uniform_int_distribution<T>`: integers in `[a, b]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UniformIntDistribution<T> {
    a: T,
    b: T,
}

impl<T: UniformInt> UniformIntDistribution<T> {
    /// `uniform_int_distribution(a, b)`.
    pub fn new(a: T, b: T) -> Self {
        UniformIntDistribution { a, b }
    }

    /// `a()`.
    pub fn a(&self) -> T {
        self.a
    }

    /// `b()`.
    pub fn b(&self) -> T {
        self.b
    }

    /// `operator()(g)`: `uniform_int::_Eval`.
    pub fn sample<G: Urbg + ?Sized>(&self, g: &mut G) -> T {
        let w = T::WIDTH;
        let umask = width_mask(w);
        let adjust = |u: u64| if T::SIGNED { u ^ (1u64 << (w - 1)) } else { u };
        let umin = adjust(self.a.to_unsigned());
        let umax = adjust(self.b.to_unsigned());
        let rng = RngV2::new::<G>(w);
        // `_Umax - _Umin == static_cast<_Uty>(-1)`, in int for the 16-bit types
        let full = if w < 32 {
            umax as i64 - umin as i64 == umask as i64
        } else {
            umax.wrapping_sub(umin) & umask == umask
        };
        let uret = if full {
            rng.all_bits(g) & umask
        } else {
            rng.below(g, umax.wrapping_sub(umin).wrapping_add(1) & umask)
        };
        T::from_unsigned(adjust(uret.wrapping_add(umin) & umask))
    }
}

impl<T: UniformInt> Default for UniformIntDistribution<T> {
    /// `uniform_int_distribution()`: `[0, max]`.
    fn default() -> Self {
        UniformIntDistribution {
            a: T::from_unsigned(0),
            b: T::MAX,
        }
    }
}

/// A floating-point type `uniform_real_distribution` takes.
pub trait UniformReal: Copy + PartialOrd {
    /// `_NRAND(g, T)`: `generate_canonical<T, digits>(g)`.
    fn canonical<G: Urbg + ?Sized>(g: &mut G) -> Self;
    /// `nrand * (b - a) + a`, as the C++ writes it (no fused multiply-add).
    fn scale(nrand: Self, a: Self, b: Self) -> Self;
    /// 0 and 1.
    const ZERO: Self;
    /// 1.
    const ONE: Self;
}

impl UniformReal for f64 {
    fn canonical<G: Urbg + ?Sized>(g: &mut G) -> f64 {
        generate_canonical_f64(g, 53)
    }
    fn scale(nrand: f64, a: f64, b: f64) -> f64 {
        nrand * (b - a) + a
    }
    const ZERO: f64 = 0.0;
    const ONE: f64 = 1.0;
}

impl UniformReal for f32 {
    fn canonical<G: Urbg + ?Sized>(g: &mut G) -> f32 {
        generate_canonical_f32(g, 24)
    }
    fn scale(nrand: f32, a: f32, b: f32) -> f32 {
        nrand * (b - a) + a
    }
    const ZERO: f32 = 0.0;
    const ONE: f32 = 1.0;
}

/// `std::uniform_real_distribution<T>`: reals in `[a, b)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UniformRealDistribution<T> {
    a: T,
    b: T,
}

impl<T: UniformReal> UniformRealDistribution<T> {
    /// `uniform_real_distribution(a, b)`.
    pub fn new(a: T, b: T) -> Self {
        UniformRealDistribution { a, b }
    }

    /// `a()`.
    pub fn a(&self) -> T {
        self.a
    }

    /// `b()`.
    pub fn b(&self) -> T {
        self.b
    }

    /// `operator()(g)`: `generate_canonical(g) * (b - a) + a`.
    pub fn sample<G: Urbg + ?Sized>(&self, g: &mut G) -> T {
        T::scale(T::canonical(g), self.a, self.b)
    }
}

impl<T: UniformReal> Default for UniformRealDistribution<T> {
    /// `uniform_real_distribution()`: `[0, 1)`.
    fn default() -> Self {
        UniformRealDistribution {
            a: T::ZERO,
            b: T::ONE,
        }
    }
}

/// `_Rng_from_urng<ptrdiff_t, Engine>(g)(index)`: a value in `[0, index)` by whole engine words
/// and rejection by division (`Udiff` is 64 bits).
fn rng_from_urng<G: Urbg + ?Sized>(g: &mut G, index: u64) -> u64 {
    let bits = G::BITS;
    let bmask = width_mask(bits);
    loop {
        let mut ret: u64 = 0;
        let mut mask: u64 = 0;
        while mask < index - 1 {
            ret = (ret << (bits - 1)) << 1;
            ret |= g.next_u64();
            mask = (mask << (bits - 1)) << 1;
            mask |= bmask;
        }
        if ret / index < mask / index || mask % index == index - 1 {
            return ret % index;
        }
    }
}

/// `std::shuffle(first, last, g)`: for each place from the second on, swaps in the element at a
/// random place at or before it.
pub fn shuffle<T, G: Urbg + ?Sized>(v: &mut [T], g: &mut G) {
    for target in 1..v.len() {
        let off = rng_from_urng(g, target as u64 + 1) as usize;
        if off != target {
            v.swap(target, off);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_10000th_outputs_are_the_standards() {
        // The C++ standard's check values ([rand.predef]).
        let mut g = Mt19937::default();
        g.discard(9999);
        assert_eq!(g.next(), 4_123_659_995);
        let mut h = Mt19937_64::default();
        h.discard(9999);
        assert_eq!(h.next(), 9_981_545_732_273_789_042);
    }

    #[test]
    fn a_one_value_range_still_takes_an_engine_call() {
        let mut g = Mt19937::new(1);
        let mut h = Mt19937::new(1);
        assert_eq!(UniformIntDistribution::new(100, 100).sample(&mut g), 100);
        h.next();
        assert_eq!(g.next(), h.next());
    }
}
