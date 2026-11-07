// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Eigen code: the reductions of Eigen/src/Core/Redux.h (redux_traits, the vectorised and
// plain linear unrollers), the SSE2 predux of Eigen/src/Core/arch/SSE/Reductions.h, the inner
// product of Eigen/src/Core/InnerProduct.h, the index searches of Eigen/src/Core/FindCoeff.h, the
// traversals of Eigen/src/Core/AssignEvaluator.h and the coefficient-based product of
// Eigen/src/Core/ProductEvaluators.h, of Eigen 5.0.1, translated on 2026-11-07. Their copyright is
// the Eigen authors' (Benoit Jacob, Gael Guennebaud and the Eigen contributors), under the Mozilla
// Public License, version 2.0 (MPL-2.0; licenses/MPL-2.0.txt).
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of
// the MPL was not distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.
// (Exhibit A of the MPL-2.0: the Eigen code here stays under it; the rest of the file is
// sketch-orca's, under the AGPL-3.0, as the MPL's section 3.3 allows.)

//! The orders in which Eigen 5.0.1, compiled by MSVC 14.42 with SSE2 and no FMA, adds up the
//! coefficients of a fixed-size reduction, a dot product and a small matrix product. Floating-point
//! addition is not associative, so these orders are part of every result.
//!
//! - [`redux`] (`sum`, `squaredNorm`, `minCoeff`, `maxCoeff`, Redux.h with complete unrolling):
//!   when the expression has packet access and at least one whole SSE2 packet fits, the packets
//!   combine in a tree, `predux` folds the lanes (`a0 + a1` for two lanes, `(a0 + a2) + (a1 + a3)`
//!   for four), and the coefficients past the last whole packet combine in a tree of their own,
//!   joined last. Otherwise the coefficients combine in a tree: each half first, the first half the
//!   smaller. So `squaredNorm` of a `Vec3d` is `(x0² + x1²) + x2²` and of a `Vec3f` (one packet is
//!   4 floats) `x0² + (x1² + x2²)`.
//! - [`dot`] (InnerProduct.h): with packet access and at least one packet, up to four packet
//!   accumulators, joined `p0 + (p1 + (p2 + p3))`, then `predux`, then the remaining products
//!   added one by one; otherwise `a0*b0`, then each next product added on. So a dot product of
//!   2 or 3 coefficients is always left to right, and of a `Vec4d` `(a0b0 + a2b2) + (a1b1 + a3b3)`.
//! - [`find_coeff`] (`minCoeff(&index)`, `maxCoeff(&index)`, FindCoeff.h): a scan by whole packets,
//!   then one by one, which gives the first extreme coefficient as a plain scan would, except where
//!   a NaN shares a packet with a better coefficient.
//! - [`product_coeff`] (ProductEvaluators.h, `LazyCoeffBasedProductMode`): one coefficient of a
//!   small fixed-size product, either by the packet path (`etor_product_packet_impl<ColMajor>`:
//!   the first product, then each next one added on, left to right) or by the coefficient path
//!   (the sum of `lhs.row(r)` times `rhs.col(c)`, a [`redux`] without packet access, so a tree).
//!   [`packet_rows`] says which rows of each column the packet path computes (AssignEvaluator.h's
//!   traversal for a product whose lhs is column-major with packet access): all of them when the
//!   rows fill whole packets, the leading whole packets when at least one fits (linear or slice
//!   vectorisation, with the rest by the coefficient path), none otherwise. A 3-row `double` result
//!   so takes rows 0 and 1 left to right and row 2 as `t0 + (t1 + t2)`; a 4-row one all rows left
//!   to right; a 3-row `float` one all rows as `t0 + (t1 + t2)`.

use crate::scalar::Scalar;

/// `predux` of one SSE2 packet (Reductions.h): `op(a0, a1)` for two lanes and
/// `op(op(a0, a2), op(a1, a3))` for four, the operands in that order.
fn predux<T: Copy>(lanes: &[T], op: &impl Fn(T, T) -> T) -> T {
    match lanes.len() {
        2 => op(lanes[0], lanes[1]),
        4 => op(op(lanes[0], lanes[2]), op(lanes[1], lanes[3])),
        n => panic!("no SSE2 packet has {n} lanes"),
    }
}

/// `redux_novec_linear_unroller`: the first `len/2` coefficients, then the rest, combined.
fn novec_tree<T: Copy>(c: &[T], op: &impl Fn(T, T) -> T) -> T {
    if c.len() == 1 {
        c[0]
    } else {
        let half = c.len() / 2;
        op(novec_tree(&c[..half], op), novec_tree(&c[half..], op))
    }
}

/// `redux_vec_linear_unroller`: a tree of `count` packets from packet `start`, lane by lane.
fn vec_tree<T: Copy>(
    c: &[T],
    start: usize,
    count: usize,
    packet: usize,
    op: &impl Fn(T, T) -> T,
) -> Vec<T> {
    if count == 1 {
        c[start * packet..(start + 1) * packet].to_vec()
    } else {
        let half = count / 2;
        let a = vec_tree(c, start, half, packet, op);
        let b = vec_tree(c, start + half, count - half, packet, op);
        a.into_iter().zip(b).map(|(x, y)| op(x, y)).collect()
    }
}

/// Eigen's `redux(func)` of the coefficients `c` (in storage order) of a fixed-size expression,
/// fully unrolled (Redux.h): `vectorized` is whether the expression has packet access and linear
/// access (a plain vector, or a coefficient-wise expression of such; not a strided row of a
/// column-major matrix), `packet` the scalar's SSE2 packet size. `op(a, b)` is the functor:
/// `a + b` for `sum`, `std::min(a, b)` for `minCoeff`, `std::max(a, b)` for `maxCoeff`.
pub fn redux<T: Copy>(c: &[T], packet: usize, vectorized: bool, op: impl Fn(T, T) -> T) -> T {
    redux_packets(c, packet, vectorized, &op, &op)
}

/// [`redux`] for a functor whose packet operation can differ from its scalar one in a lane:
/// `packet_op` joins whole packets lane by lane (`func.packetOp`; `pmin<Packet2l>` for an
/// `int64_t` `minCoeff`), `op` folds the lanes of the result (`predux_min` of a `Packet2l` stores
/// the lanes and takes `std::min`) and combines the coefficients past the last whole packet.
pub fn redux_packets<T: Copy>(
    c: &[T],
    packet: usize,
    vectorized: bool,
    packet_op: impl Fn(T, T) -> T,
    op: impl Fn(T, T) -> T,
) -> T {
    assert!(!c.is_empty(), "Eigen reduces no empty expression");
    let n = c.len();
    if vectorized && n >= packet {
        let packets = n / packet;
        let lanes = vec_tree(c, 0, packets, packet, &packet_op);
        let res = predux(&lanes, &op);
        if packets * packet == n {
            res
        } else {
            op(res, novec_tree(&c[packets * packet..], &op))
        }
    } else {
        novec_tree(c, &op)
    }
}

/// `a.dot(b)` (InnerProduct.h): `vectorized` is whether both operands have packet access.
pub fn dot<T: Scalar>(a: &[T], b: &[T], vectorized: bool) -> T {
    assert_eq!(
        a.len(),
        b.len(),
        "a dot product of vectors of different sizes"
    );
    let n = a.len();
    if n == 0 {
        return T::ZERO;
    }
    let p = T::PACKET;
    let term = |k: usize| a[k].mul(b[k]);
    if !vectorized || n < p {
        let mut r = term(0);
        for k in 1..n {
            r = term(k).add(r);
        }
        return r;
    }
    let packet_end = n / p * p;
    let quad_end = n / (4 * p) * (4 * p);
    let num_packets = n / p;
    let num_rem = (packet_end - quad_end) / p;
    let load = |k: usize| -> Vec<T> { (k..k + p).map(term).collect() };
    let acc = |acc: &mut Vec<T>, k: usize| {
        for (i, x) in acc.iter_mut().enumerate() {
            *x = term(k + i).add(*x);
        }
    };
    let add = |x: &[T], y: &[T]| -> Vec<T> { x.iter().zip(y).map(|(a, b)| a.add(*b)).collect() };
    let mut p0 = load(0);
    let mut p1 = if num_packets >= 2 {
        load(p)
    } else {
        Vec::new()
    };
    let mut p2 = if num_packets >= 3 {
        load(2 * p)
    } else {
        Vec::new()
    };
    if num_packets >= 4 {
        let mut p3 = load(3 * p);
        let mut k = 4 * p;
        while k < quad_end {
            acc(&mut p0, k);
            acc(&mut p1, k + p);
            acc(&mut p2, k + 2 * p);
            acc(&mut p3, k + 3 * p);
            k += 4 * p;
        }
        if num_rem >= 1 {
            acc(&mut p0, quad_end);
        }
        if num_rem >= 2 {
            acc(&mut p1, quad_end + p);
        }
        if num_rem == 3 {
            acc(&mut p2, quad_end + 2 * p);
        }
        p2 = add(&p2, &p3);
    }
    if num_packets >= 3 {
        p1 = add(&p1, &p2);
    }
    if num_packets >= 2 {
        p0 = add(&p0, &p1);
    }
    let mut r = predux(&p0, &|x: T, y: T| x.add(y));
    for k in packet_end..n {
        r = term(k).add(r);
    }
    r
}

/// `minCoeff(&index)` or `maxCoeff(&index)` of the coefficients `c` of a vector, with the default
/// `PropagateFast` (FindCoeff.h, `find_coeff_loop<.., Linear, ..>`): the extreme coefficient and
/// its index. `better(incumbent, candidate)` is the functor's comparison (`candidate < incumbent`
/// for the minimum, `candidate > incumbent` for the maximum), `packet_better` the same comparison
/// in a packet lane (`comparePacket`: `pcmp_lt`, the emulated one for `int64_t`), `op` its packet
/// reduction (`std::min` or `std::max`), `vectorized` whether the expression has packet access.
///
/// With packet access and at least one whole packet, the result starts as `c[0]`. Each packet
/// holding a coefficient better than the result (`predux_any` of the lane comparisons) makes its
/// `predux` the result and its first coefficient the index; the coefficients after the last whole
/// packet are compared one by one; when a packet gave the last result, the index is searched again
/// in that packet, from its first coefficient. Otherwise one scan from `c[0]`. The two agree but
/// where a NaN shares a packet with a better coefficient: the packet's `predux` can be the NaN,
/// which no later coefficient beats, and the second search then stays on the packet's first
/// coefficient; or where an `int64_t` lane comparison is wrong (past 2^31).
pub fn find_coeff<T: Copy>(
    c: &[T],
    packet: usize,
    vectorized: bool,
    packet_better: impl Fn(T, T) -> bool,
    better: impl Fn(T, T) -> bool,
    op: impl Fn(T, T) -> T,
) -> (T, usize) {
    assert!(!c.is_empty(), "Eigen searches no empty expression");
    let n = c.len();
    let mut result = c[0];
    let mut index = 0;
    if !vectorized || n < packet {
        for (k, &x) in c.iter().enumerate() {
            if better(result, x) {
                index = k;
                result = x;
            }
        }
        return (result, index);
    }
    let packet_end = n / packet * packet;
    let mut check_packet = false;
    for (p, lanes) in c[..packet_end].chunks_exact(packet).enumerate() {
        if lanes.iter().any(|&x| packet_better(result, x)) {
            index = p * packet;
            result = predux(lanes, &op);
            check_packet = true;
        }
    }
    for (k, &x) in c.iter().enumerate().skip(packet_end) {
        if better(result, x) {
            index = k;
            result = x;
            check_packet = false;
        }
    }
    if check_packet {
        let start = index;
        result = c[start];
        for (k, &x) in c.iter().enumerate().skip(start).take(packet) {
            if better(result, x) {
                index = k;
                result = x;
            }
        }
    }
    (result, index)
}

/// How many leading rows of each column of a `rows` x `cols` destination a lazy coefficient-based
/// product computes by the packet path, its lhs column-major with packet access
/// (AssignEvaluator.h): all rows when they fill whole packets (inner vectorisation), the whole
/// packets when at least one fits (linear vectorisation of a vector, slice vectorisation of a
/// matrix, unrolled), none when not even one packet fits. `cols == 1` is a vector.
pub fn packet_rows(rows: usize, packet: usize) -> usize {
    if rows.is_multiple_of(packet) {
        rows
    } else if rows >= packet {
        rows / packet * packet
    } else {
        0
    }
}

/// One coefficient of a small product `lhs * rhs`, the `k` terms `lhs(r, i) * rhs(i, c)` given by
/// `term(i)`: by the packet path (`packet == true`) the first term, then each next one added on;
/// by the coefficient path a tree ([`redux`] without packet access).
pub fn product_coeff<T: Scalar>(k: usize, packet: bool, term: impl Fn(usize) -> T) -> T {
    if packet {
        let mut acc = term(0);
        for i in 1..k {
            acc = term(i).add(acc);
        }
        acc
    } else {
        let terms: Vec<T> = (0..k).map(term).collect();
        redux(&terms, T::PACKET, false, |a: T, b: T| a.add(b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Values whose sums round differently in each order, so each test pins its order.
    const A: f64 = 1.0;
    const B: f64 = 1e-16;
    const C: f64 = 1e-16;

    #[test]
    fn three_doubles_sum_packet_first() {
        // (A + B) + C rounds both to A; A + (B + C) keeps 2e-16.
        assert_eq!(redux(&[A, B, C], 2, true, |x, y| x + y), (A + B) + C);
        assert_eq!(redux(&[A, B, C], 2, false, |x, y| x + y), A + (B + C));
        assert_ne!((A + B) + C, A + (B + C));
    }

    #[test]
    fn four_lanes_fold_across() {
        let c = [A, B, -A, C];
        assert_eq!(redux(&c, 2, true, |x, y| x + y), (A + -A) + (B + C));
        assert_eq!(redux(&c, 4, true, |x, y| x + y), (A + -A) + (B + C));
        assert_eq!(redux(&c, 4, false, |x, y| x + y), (A + B) + (-A + C));
    }

    #[test]
    fn dots_of_two_and_three_go_left_to_right() {
        assert_eq!(dot(&[A, B, C], &[1.0, 1.0, 1.0], true), (A + B) + C);
        assert_eq!(dot(&[A, B, -A, C], &[1.0; 4], true), (A + -A) + (B + C));
        assert_eq!(dot(&[A, B, -A, C], &[1.0; 4], false), ((A + B) + -A) + C);
    }

    #[test]
    fn a_nan_beside_a_better_coefficient_stops_the_index_search() {
        let max = |a: f64, b: f64| if a < b { b } else { a };
        let better = |inc: f64, cand: f64| cand > inc;
        let c = [1.0, 0.0, f64::NAN, 5.0];
        // A plain scan skips the NaN and finds 5.0; the packet [NaN, 5.0] reduces to the NaN.
        assert_eq!(find_coeff(&c, 2, false, better, better, max).1, 3);
        let (v, i) = find_coeff(&c, 2, true, better, better, max);
        assert!(v.is_nan());
        assert_eq!(i, 2);
        // Ties keep the first coefficient either way.
        assert_eq!(
            find_coeff(&[2.0, 7.0, 7.0, 1.0], 2, true, better, better, max),
            (7.0, 1)
        );
        assert_eq!(
            find_coeff(&[7.0, 1.0, 7.0], 2, true, better, better, max),
            (7.0, 0)
        );
    }

    #[test]
    fn int64_lanes_compare_as_sse2_emulates() {
        use crate::scalar::Scalar;
        let max = |a: i64, b: i64| if a < b { b } else { a };
        let packet_better = |inc: i64, cand: i64| inc.packet_lt(cand);
        let better = |inc: i64, cand: i64| cand > inc;
        // In the packet [2^32, 2^32 + 2^31] the larger lane does not compare greater.
        let c = [0x1_0000_0000_i64, 0x1_8000_0000];
        assert_eq!(find_coeff(&c, 2, false, better, better, max), (c[1], 1));
        assert_eq!(
            find_coeff(&c, 2, true, packet_better, better, max),
            (c[0], 0)
        );
        let pmax = |a: i64, b: i64| a.packet_max(b);
        let c4 = [0x1_0000_0000_i64, 0, 0x1_8000_0000, 0];
        assert_eq!(redux_packets(&c4, 2, true, pmax, max), 0x1_0000_0000);
        assert_eq!(redux(&c4, 2, true, max), 0x1_8000_0000);
    }

    #[test]
    fn packet_rows_follow_the_traversal() {
        assert_eq!(packet_rows(3, 2), 2);
        assert_eq!(packet_rows(4, 2), 4);
        assert_eq!(packet_rows(3, 4), 0);
        assert_eq!(packet_rows(4, 4), 4);
        assert_eq!(packet_rows(2, 2), 2);
    }
}
