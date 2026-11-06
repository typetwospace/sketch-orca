// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Microsoft STL code: this file is translated from stl/inc/utility (min and max of two),
// stl/inc/xutility (_Max_element_unchecked, _Min_element_unchecked, max_element, min_element, the
// initializer-list max and min, _Is_min_max_optimization_safe), stl/inc/algorithm
// (_Minmax_element_unchecked, minmax_element, clamp) and stl/src/vector_algorithms.cpp
// (__std_minmax_element_impl, __std_minmax_element_disp, _Min_tail, _Max_tail, _Both_tail and the
// _Minmax_traits_f and _Minmax_traits_d SSE4.2 and AVX2 traits) of microsoft/STL at the tag
// vs-2022-17.12 (commit 91e425587a0b9b5ac0d1005844c661fee2e2813b), MSVC 14.42's min, max and
// element searches, lane for lane where they are vectorised, on 2026-11-06. Its copyright holder
// is Microsoft Corporation, and it is under Apache-2.0 WITH LLVM-exception
// (licenses/Apache-2.0.txt and licenses/LLVM-exception.txt).

//! `std::min`, `std::max`, `std::clamp` and the element searches, with MSVC's NaN behaviour.
//!
//! - [`max`] is `a < b ? b : a` and [`min`] is `b < a ? b : a`: with a NaN or with `-0.0` and
//!   `0.0` they return the first argument. Rust's `f64::max` and `f64::min` (IEEE maxNum and
//!   minNum) do not, so `clippy.toml` sends the engine here.
//! - [`clamp`] is the branchless form MSVC uses for scalars with `std::less`: `v < lo ? lo : v`,
//!   then `hi < v ? hi : that`. [`clamp_by`] is the form for any other comparator.
//! - [`max_element_by`], [`min_element_by`] and [`minmax_element_by`] are the plain loops, which
//!   MSVC runs for every comparator other than `std::less` and for every non-arithmetic type.
//! - For `float` and `double` with `std::less`, MSVC 14.42 runs `std::max_element`,
//!   `std::min_element`, `std::minmax_element` and the initializer-list `std::max({...})` and
//!   `std::min({...})` through `vector_algorithms.cpp`, which keeps a minimum and a maximum per
//!   SIMD lane and then reduces across the lanes. Without NaNs that gives the plain loop's
//!   answer; with NaNs it does not, and the answer depends on the lane count. [`max_element_f64`]
//!   and the other float functions here follow it lane for lane, as an AVX2 processor runs it
//!   (any x64 processor since 2013, and the oracle's); [`VectorIsa`] selects the SSE4.2 or
//!   scalar dispatch that older processors take. The program gives the same answer on every
//!   machine, which the C++ engine does not.
//!
//! For integers, `std::max_element` and the rest give the plain loop's answer however they are
//! run, so [`max_element`] and friends require `Ord`.

/// `std::max(a, b)`: `a < b ? b : a`; the first argument when they are unordered or equal.
#[inline]
pub fn max<T: PartialOrd>(a: T, b: T) -> T {
    if a < b { b } else { a }
}

/// `std::min(a, b)`: `b < a ? b : a`; the first argument when they are unordered or equal.
#[inline]
pub fn min<T: PartialOrd>(a: T, b: T) -> T {
    if b < a { b } else { a }
}

/// `std::max(a, b, pred)`: `pred(a, b) ? b : a`.
#[inline]
pub fn max_by<T, F: FnOnce(&T, &T) -> bool>(a: T, b: T, less: F) -> T {
    if less(&a, &b) { b } else { a }
}

/// `std::min(a, b, pred)`: `pred(b, a) ? b : a`.
#[inline]
pub fn min_by<T, F: FnOnce(&T, &T) -> bool>(a: T, b: T, less: F) -> T {
    if less(&b, &a) { b } else { a }
}

/// `std::minmax(a, b)`: `(b, a)` if `b < a`, else `(a, b)`.
#[inline]
pub fn minmax<T: PartialOrd>(a: T, b: T) -> (T, T) {
    if b < a { (b, a) } else { (a, b) }
}

/// `std::clamp(v, lo, hi)` for an arithmetic type with `std::less`: MSVC's branchless form,
/// `v < lo ? lo : v`, then `hi < v ? hi : that` (both comparisons always made).
#[inline]
pub fn clamp<T: PartialOrd>(v: T, lo: T, hi: T) -> T {
    let below = v < lo;
    let above = hi < v;
    let tmp = if below { lo } else { v };
    if above { hi } else { tmp }
}

/// `std::clamp(v, lo, hi, pred)`: `pred(hi, v)` gives `hi`, else `pred(v, lo)` gives `lo`, else
/// `v`.
#[inline]
pub fn clamp_by<T, F: FnMut(&T, &T) -> bool>(v: T, lo: T, hi: T, mut less: F) -> T {
    if less(&hi, &v) {
        return hi;
    }
    if less(&v, &lo) {
        return lo;
    }
    v
}

/// `std::max_element(first, last, pred)`: the first element no other goes after; 0 for an
/// empty slice (`last`, which is `first`).
pub fn max_element_by<T, F: FnMut(&T, &T) -> bool>(v: &[T], mut less: F) -> usize {
    let mut found = 0usize;
    for i in 1..v.len() {
        if less(&v[found], &v[i]) {
            found = i;
        }
    }
    found
}

/// `std::min_element(first, last, pred)`: the first element no other goes before.
pub fn min_element_by<T, F: FnMut(&T, &T) -> bool>(v: &[T], mut less: F) -> usize {
    let mut found = 0usize;
    for i in 1..v.len() {
        if less(&v[i], &v[found]) {
            found = i;
        }
    }
    found
}

/// `std::minmax_element(first, last, pred)`: MSVC's pairwise loop, giving the first smallest
/// and the last largest element.
pub fn minmax_element_by<T, F: FnMut(&T, &T) -> bool>(v: &[T], mut less: F) -> (usize, usize) {
    let (mut lo, mut hi) = (0usize, 0usize);
    let last = v.len();
    if last == 0 {
        return (0, 0);
    }
    let mut first = 0usize;
    loop {
        first += 1;
        if first == last {
            break;
        }
        let next = first + 1;
        if next == last {
            if less(&v[first], &v[lo]) {
                lo = first;
            } else if !less(&v[first], &v[hi]) {
                hi = first;
            }
        } else {
            if less(&v[next], &v[first]) {
                if less(&v[next], &v[lo]) {
                    lo = next;
                }
                if !less(&v[first], &v[hi]) {
                    hi = first;
                }
            } else {
                if less(&v[first], &v[lo]) {
                    lo = first;
                }
                if !less(&v[next], &v[hi]) {
                    hi = next;
                }
            }
            first = next;
        }
    }
    (lo, hi)
}

/// `std::max_element(first, last)` for a totally ordered type.
pub fn max_element<T: Ord>(v: &[T]) -> usize {
    max_element_by(v, |a, b| a < b)
}

/// `std::min_element(first, last)` for a totally ordered type.
pub fn min_element<T: Ord>(v: &[T]) -> usize {
    min_element_by(v, |a, b| a < b)
}

/// `std::minmax_element(first, last)` for a totally ordered type.
pub fn minmax_element<T: Ord>(v: &[T]) -> (usize, usize) {
    minmax_element_by(v, |a, b| a < b)
}

/// `std::max({a, b, ...})` for a totally ordered type: the first greatest.
///
/// # Panics
/// On an empty slice (an empty initializer list is undefined in C++).
pub fn max_of<T: Ord + Clone>(v: &[T]) -> T {
    assert!(!v.is_empty(), "max of an empty list");
    v[max_element(v)].clone()
}

/// `std::min({a, b, ...})` for a totally ordered type: the first smallest.
///
/// # Panics
/// On an empty slice.
pub fn min_of<T: Ord + Clone>(v: &[T]) -> T {
    assert!(!v.is_empty(), "min of an empty list");
    v[min_element(v)].clone()
}

/// The `vector_algorithms.cpp` dispatch MSVC's floating-point element searches take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VectorIsa {
    /// No SSE4.2: the scalar tails only.
    Scalar,
    /// SSE4.2 without AVX2: 16-byte vectors.
    Sse42,
    /// AVX2: 32-byte vectors for ranges of 32 bytes or more, 16-byte ones below that.
    Avx2,
}

/// The dispatch this crate follows by default: AVX2, as on every x64 processor since 2013 and on
/// the machine the oracle runs on.
pub const DEFAULT_ISA: VectorIsa = VectorIsa::Avx2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Min,
    Max,
    Both,
}

/// A float lane of `vector_algorithms.cpp`'s traits.
trait Lane: Copy + PartialOrd {
    const SIZE: usize;
    const INIT_MIN: Self;
    const INIT_MAX: Self;
    /// The vertical index lanes wrap at `2^32` vectors for `float` (`_Portion_max`); `double`
    /// has none.
    const PORTION_MAX: Option<usize>;
    /// The all-ones vertical index (`_All_ones` for the lane width).
    const IDX_ALL_ONES: u64;
}

impl Lane for f64 {
    const SIZE: usize = 8;
    const INIT_MIN: Self = f64::INFINITY;
    const INIT_MAX: Self = f64::NEG_INFINITY;
    const PORTION_MAX: Option<usize> = None;
    const IDX_ALL_ONES: u64 = u64::MAX;
}

impl Lane for f32 {
    const SIZE: usize = 4;
    const INIT_MIN: Self = f32::INFINITY;
    const INIT_MAX: Self = f32::NEG_INFINITY;
    const PORTION_MAX: Option<usize> = Some(1usize << 32);
    const IDX_ALL_ONES: u64 = u32::MAX as u64;
}

/// `minps` and `minpd`: `a < b ? a : b`.
#[inline]
fn simd_min<T: Lane>(a: T, b: T) -> T {
    if a < b { a } else { b }
}

/// `maxps` and `maxpd`: `a > b ? a : b`.
#[inline]
fn simd_max<T: Lane>(a: T, b: T) -> T {
    if a > b { a } else { b }
}

/// `_H_func`: each step combines every lane with the lane at its index XOR 1, 2, 4..., the
/// partner as the first operand; the result keeps all lanes, which NaNs make differ.
fn horizontal<T: Lane>(v: &[T], f: fn(T, T) -> T) -> Vec<T> {
    let mut h = v.to_vec();
    let mut step = 1;
    while step < h.len() {
        let prev = h.clone();
        for (i, x) in h.iter_mut().enumerate() {
            *x = f(prev[i ^ step], prev[i]);
        }
        step <<= 1;
    }
    h
}

/// `__std_minmax_element_disp` and `__std_minmax_element_impl` for `float` and `double`, lane for
/// lane. Returns (min position, max position); only the mode's half is meaningful.
fn element_impl<T: Lane>(v: &[T], mode: Mode, isa: VectorIsa) -> (usize, usize) {
    let bytes = v.len() * T::SIZE;
    let lanes = if bytes >= 32 && isa == VectorIsa::Avx2 {
        32 / T::SIZE
    } else if bytes >= 16 && isa != VectorIsa::Scalar {
        16 / T::SIZE
    } else {
        0
    };
    let has_min = mode != Mode::Max;
    let has_max = mode != Mode::Min;
    let (mut res_min, mut res_max) = (0usize, 0usize);
    let mut cur_min_val = T::INIT_MIN;
    let mut cur_max_val = T::INIT_MAX;
    let mut first = 0usize;

    if lanes > 0 {
        let idx_mask = T::IDX_ALL_ONES;
        let clamp_portion = |p: usize| match T::PORTION_MAX {
            Some(max_vectors) => p.min(max_vectors * lanes),
            None => p,
        };
        let mut base = 0usize;
        let mut stop_at = first + clamp_portion((v.len() - first) / lanes * lanes);
        let load = |at: usize| v[at..at + lanes].to_vec();
        let mut vals_min = load(first);
        let mut idx_min = vec![0u64; lanes];
        let mut vals_max = vals_min.clone();
        let mut idx_max = vec![0u64; lanes];
        let mut cur_idx = 0u64;
        loop {
            first += lanes;
            cur_idx = cur_idx.wrapping_add(1) & idx_mask;
            if first != stop_at {
                let cur = load(first);
                for l in 0..lanes {
                    if has_min {
                        // _Is_less = _Cmp_gt(_Cur_vals_min, _Cur_vals); _Min = minps(_Cur_vals, _Cur_vals_min)
                        if vals_min[l] > cur[l] {
                            idx_min[l] = cur_idx;
                        }
                        vals_min[l] = simd_min(cur[l], vals_min[l]);
                    }
                    match mode {
                        Mode::Max => {
                            if cur[l] > vals_max[l] {
                                idx_max[l] = cur_idx;
                            }
                            vals_max[l] = simd_max(cur[l], vals_max[l]);
                        }
                        Mode::Both => {
                            // The last occurrence: the index stays only while the kept value is
                            // greater (`_Blend(_Cur_idx, _Cur_idx_max, _Is_less)`).
                            let keep = vals_max[l] > cur[l];
                            if !keep {
                                idx_max[l] = cur_idx;
                            }
                            vals_max[l] = simd_max(vals_max[l], cur[l]);
                        }
                        Mode::Min => {}
                    }
                }
            } else {
                if has_min {
                    let h = horizontal(&vals_min, simd_min::<T>);
                    let h_min_val = h[0];
                    if h_min_val < cur_min_val {
                        cur_min_val = h_min_val;
                        let eq: Vec<bool> = (0..lanes).map(|l| h[l] == vals_min[l]).collect();
                        let idx_val: Vec<u64> = (0..lanes)
                            .map(|l| if eq[l] { idx_min[l] } else { idx_mask })
                            .collect();
                        let smallest = idx_val.iter().copied().min().unwrap_or(idx_mask);
                        let lane = (0..lanes)
                            .find(|&l| eq[l] && idx_val[l] == smallest)
                            .unwrap_or(0);
                        res_min = base + idx_min[lane] as usize * lanes + lane;
                    }
                }
                if has_max {
                    let h = horizontal(&vals_max, simd_max::<T>);
                    let h_max_val = h[0];
                    let update = match mode {
                        Mode::Both => cur_max_val <= h_max_val,
                        _ => cur_max_val < h_max_val,
                    };
                    if update {
                        cur_max_val = h_max_val;
                        let eq: Vec<bool> = (0..lanes).map(|l| h[l] == vals_max[l]).collect();
                        let lane = if mode == Mode::Both {
                            let idx_val: Vec<u64> = (0..lanes)
                                .map(|l| if eq[l] { idx_max[l] } else { 0 })
                                .collect();
                            let greatest = idx_val.iter().copied().fold(0, max);
                            (0..lanes)
                                .rev()
                                .find(|&l| eq[l] && idx_val[l] == greatest)
                                .unwrap_or(0)
                        } else {
                            let idx_val: Vec<u64> = (0..lanes)
                                .map(|l| if eq[l] { idx_max[l] } else { idx_mask })
                                .collect();
                            let smallest = idx_val.iter().copied().min().unwrap_or(idx_mask);
                            (0..lanes)
                                .find(|&l| eq[l] && idx_val[l] == smallest)
                                .unwrap_or(0)
                        };
                        res_max = base + idx_max[lane] as usize * lanes + lane;
                    }
                }
                if T::PORTION_MAX.is_some() {
                    let portion = clamp_portion((v.len() - first) / lanes * lanes);
                    if portion == 0 {
                        break;
                    }
                    stop_at += portion;
                    base = first;
                    let cur = load(first);
                    if has_min {
                        vals_min = cur.clone();
                        idx_min = vec![0u64; lanes];
                    }
                    if has_max {
                        vals_max = cur;
                        idx_max = vec![0u64; lanes];
                    }
                    cur_idx = 0;
                } else {
                    break;
                }
            }
        }
    }

    // _Min_tail, _Max_tail and _Both_tail
    for (i, &x) in v.iter().enumerate().skip(first) {
        if has_min && x < cur_min_val {
            res_min = i;
            cur_min_val = x;
        }
        match mode {
            Mode::Max => {
                if cur_max_val < x {
                    res_max = i;
                    cur_max_val = x;
                }
            }
            Mode::Both => {
                if cur_max_val <= x {
                    res_max = i;
                    cur_max_val = x;
                }
            }
            Mode::Min => {}
        }
    }
    (res_min, res_max)
}

/// `std::max_element` on `double`s with `std::less`, as MSVC's AVX2 dispatch finds it.
pub fn max_element_f64(v: &[f64]) -> usize {
    max_element_f64_with(v, DEFAULT_ISA)
}

/// `std::min_element` on `double`s with `std::less`, as MSVC's AVX2 dispatch finds it.
pub fn min_element_f64(v: &[f64]) -> usize {
    min_element_f64_with(v, DEFAULT_ISA)
}

/// `std::minmax_element` on `double`s with `std::less`, as MSVC's AVX2 dispatch finds it.
pub fn minmax_element_f64(v: &[f64]) -> (usize, usize) {
    minmax_element_f64_with(v, DEFAULT_ISA)
}

/// `std::max_element` on `float`s with `std::less`, as MSVC's AVX2 dispatch finds it.
pub fn max_element_f32(v: &[f32]) -> usize {
    max_element_f32_with(v, DEFAULT_ISA)
}

/// `std::min_element` on `float`s with `std::less`, as MSVC's AVX2 dispatch finds it.
pub fn min_element_f32(v: &[f32]) -> usize {
    min_element_f32_with(v, DEFAULT_ISA)
}

/// `std::minmax_element` on `float`s with `std::less`, as MSVC's AVX2 dispatch finds it.
pub fn minmax_element_f32(v: &[f32]) -> (usize, usize) {
    minmax_element_f32_with(v, DEFAULT_ISA)
}

/// [`max_element_f64`] under the given dispatch.
pub fn max_element_f64_with(v: &[f64], isa: VectorIsa) -> usize {
    element_impl(v, Mode::Max, isa).1
}

/// [`min_element_f64`] under the given dispatch.
pub fn min_element_f64_with(v: &[f64], isa: VectorIsa) -> usize {
    element_impl(v, Mode::Min, isa).0
}

/// [`minmax_element_f64`] under the given dispatch.
pub fn minmax_element_f64_with(v: &[f64], isa: VectorIsa) -> (usize, usize) {
    element_impl(v, Mode::Both, isa)
}

/// [`max_element_f32`] under the given dispatch.
pub fn max_element_f32_with(v: &[f32], isa: VectorIsa) -> usize {
    element_impl(v, Mode::Max, isa).1
}

/// [`min_element_f32`] under the given dispatch.
pub fn min_element_f32_with(v: &[f32], isa: VectorIsa) -> usize {
    element_impl(v, Mode::Min, isa).0
}

/// [`minmax_element_f32`] under the given dispatch.
pub fn minmax_element_f32_with(v: &[f32], isa: VectorIsa) -> (usize, usize) {
    element_impl(v, Mode::Both, isa)
}

/// `std::max({a, b, ...})` on `double`s: `*max_element`, through the vectorised search.
///
/// # Panics
/// On an empty slice.
pub fn max_of_f64(v: &[f64]) -> f64 {
    assert!(!v.is_empty(), "max of an empty list");
    v[max_element_f64(v)]
}

/// `std::min({a, b, ...})` on `double`s: `*min_element`, through the vectorised search.
///
/// # Panics
/// On an empty slice.
pub fn min_of_f64(v: &[f64]) -> f64 {
    assert!(!v.is_empty(), "min of an empty list");
    v[min_element_f64(v)]
}

/// `std::minmax({a, b, ...})` on `double`s: `*minmax_element`, through the vectorised search.
///
/// # Panics
/// On an empty slice.
pub fn minmax_of_f64(v: &[f64]) -> (f64, f64) {
    assert!(!v.is_empty(), "minmax of an empty list");
    let (lo, hi) = minmax_element_f64(v);
    (v[lo], v[hi])
}

/// `std::max({a, b, ...})` on `float`s.
///
/// # Panics
/// On an empty slice.
pub fn max_of_f32(v: &[f32]) -> f32 {
    assert!(!v.is_empty(), "max of an empty list");
    v[max_element_f32(v)]
}

/// `std::min({a, b, ...})` on `float`s.
///
/// # Panics
/// On an empty slice.
pub fn min_of_f32(v: &[f32]) -> f32 {
    assert!(!v.is_empty(), "min of an empty list");
    v[min_element_f32(v)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_argument_forms_return_the_first_when_unordered() {
        let nan = f64::NAN;
        assert!(max(nan, 1.0).is_nan());
        assert_eq!(max(1.0, nan), 1.0);
        assert!(min(nan, 1.0).is_nan());
        assert_eq!(min(1.0, nan), 1.0);
        assert!(max(-0.0f64, 0.0).is_sign_negative());
        assert!(min(0.0f64, -0.0).is_sign_positive());
    }

    #[test]
    fn the_scalar_dispatch_starts_from_minus_infinity() {
        // [NaN, 1, 2]: the tail loop starts from -inf, so it finds 2, not the NaN at 0.
        assert_eq!(
            max_element_f64_with(&[f64::NAN, 1.0, 2.0], VectorIsa::Scalar),
            2
        );
        assert_eq!(max_element_by(&[f64::NAN, 1.0, 2.0], |a, b| a < b), 0);
    }
}
