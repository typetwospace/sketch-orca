// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-exact: the exact-arithmetic records of `arrange.sop1`; orca-arrange
//! replays the rest (`parity/SOP1.md`, "arrange.sop1"), bit for bit (added 2026-11-07):
//!
//! - XI28: Boost.Multiprecision's `int128_t`: `+ - * / %`, negation, `abs`, `gcd` (Lehmer's
//!   steps, and the exceptions their wrapped products end in), the conversion to `double` and
//!   the comparisons; a remainder by zero is not recorded, as it ends the C++ process;
//! - XRAT: `boost::rational<int128_t>`: construction, the arithmetic with rationals and integers,
//!   the comparisons, negation, `abs` and `rational_cast<double>`, with their exceptions.
//!
//! The committed subset (`parity/probes/`) runs by default; `SKETCH_ORCA_PROBES_DEEP=<folder>`
//! replays the full set from that folder (`<work>/parity/probes`).

use std::fmt::{self, Debug, Formatter};

use orca_exact::{ExactError, Int128, Rational};
use orca_probe::{Mismatches, ProbeFile, Reader, load_probe};

fn file() -> ProbeFile {
    load_probe("arrange").unwrap_or_else(|e| panic!("{e}"))
}

/// The kinds of `arrange.sop1` that this file replays; orca-arrange replays the others.
const EXACT_KINDS: [&str; 2] = ["XI28", "XRAT"];

/// An `f64` compared by its bits.
#[derive(Clone, Copy)]
struct F(f64);

impl PartialEq for F {
    fn eq(&self, o: &Self) -> bool {
        self.0.to_bits() == o.0.to_bits()
    }
}

impl Debug for F {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

type R = Rational<Int128>;

/// One record's reader, the mismatch list and the record's label.
struct Case<'a, 'm> {
    r: Reader<'a>,
    m: &'m mut Mismatches,
    label: String,
}

impl Case<'_, '_> {
    fn bool(&mut self) -> bool {
        self.r.bool().unwrap()
    }
    fn str(&mut self) -> String {
        self.r.str().unwrap()
    }
    /// A value: u8 negative, u64 high and u64 low half of the magnitude.
    fn int(&mut self) -> Int128 {
        let neg = self.bool();
        let hi = self.r.u64().unwrap();
        let lo = self.r.u64().unwrap();
        Int128::from_sign_magnitude(neg, (u128::from(hi) << 64) | u128::from(lo))
    }
    fn rational(&mut self) -> R {
        let n = self.int();
        R::from_parts(n, self.int())
    }

    fn is_int(&mut self, what: &str, got: Int128) {
        let want = self.int();
        self.same(what, &got, &want);
    }
    fn is_bool(&mut self, what: &str, got: bool) {
        let want = self.bool();
        self.same(what, &got, &want);
    }
    /// A call that may throw: `false` and the value, or `true` and `what()`. True when both
    /// returned.
    fn outcome<T: PartialEq + Debug>(
        &mut self,
        what: &str,
        got: Result<T, ExactError>,
        read: impl FnOnce(&mut Self) -> T,
    ) -> bool {
        let want = if self.bool() {
            Err(self.str())
        } else {
            Ok(read(self))
        };
        let got = got.map_err(|e| e.what().to_owned());
        let both = got.is_ok() && want.is_ok();
        self.same(what, &got, &want);
        both
    }

    /// Compares one result with the C++ engine's.
    fn same<T: PartialEq + Debug>(&mut self, what: &str, got: &T, want: &T) {
        let label = &self.label;
        self.m.check(got == want, || {
            format!("{label}: {what}: {got:?} instead of {want:?}")
        });
    }
}

/// Replays every record of `kind` with `f`; a record that matched must have been read to its end.
fn replay(kind: &str, f: impl Fn(&mut Case)) {
    let file = file();
    let mut m = Mismatches::default();
    for (i, rec) in file.of_kind(kind).enumerate() {
        let before = m.count;
        let mut c = Case {
            r: rec.reader(),
            m: &mut m,
            label: format!("{kind} {i}"),
        };
        f(&mut c);
        if c.m.count == before {
            c.r.finish(&c.label).unwrap();
        }
    }
    m.assert_none(kind);
}

fn xi28(c: &mut Case) {
    let a = c.int();
    let b = c.int();
    c.is_int("a + b", a.add(b));
    c.is_int("a - b", a.sub(b));
    c.is_int("a * b", a.mul(b));
    c.outcome("a / b", a.div(b), Case::int);
    if !b.is_zero() {
        c.outcome("a % b", Ok(a.rem(b)), Case::int);
    }
    c.is_int("-a", a.neg());
    c.is_int("abs(a)", a.abs());
    c.outcome("gcd(a, b)", a.gcd(b), Case::int);
    let want = F(c.r.f64().unwrap());
    c.same("double(a)", &F(a.to_f64()), &want);
    c.is_bool("a < b", a < b);
    c.is_bool("a == b", a == b);
    c.is_bool("a > b", a > b);
}

#[test]
fn int128_matches() {
    replay("XI28", xi28);
}

fn xrat(c: &mut Case) {
    let (n1, d1, n2, d2, i) = (c.int(), c.int(), c.int(), c.int(), c.int());
    let ra = R::new(n1, d1);
    let rb = R::new(n2, d2);
    let oka = c.outcome("rational(n1, d1)", ra, Case::rational);
    let okb = c.outcome("rational(n2, d2)", rb, Case::rational);
    if !oka || !okb {
        return;
    }
    let (a, b) = (ra.unwrap(), rb.unwrap());
    c.outcome("a + b", a.add(b), Case::rational);
    c.outcome("a - b", a.sub(b), Case::rational);
    c.outcome("a * b", a.mul(b), Case::rational);
    c.outcome("a / b", a.div(b), Case::rational);
    c.outcome("a < b", a.lt(&b), Case::bool);
    c.outcome("a > b", a.gt(&b), Case::bool);
    c.outcome("a == b", Ok(a.eq_rational(&b)), Case::bool);
    c.outcome("a < i", a.lt_int(i), Case::bool);
    c.outcome("a > i", a.gt_int(i), Case::bool);
    c.outcome("a == i", Ok(a.eq_int(i)), Case::bool);
    c.outcome("a + i", Ok(a.add_int(i)), Case::rational);
    c.outcome("a - i", Ok(a.sub_int(i)), Case::rational);
    c.outcome("a * i", a.mul_int(i), Case::rational);
    c.outcome("a / i", a.div_int(i), Case::rational);
    c.outcome("-a", a.neg(), Case::rational);
    c.outcome("abs(a)", a.abs(), Case::rational);
    let want = F(c.r.f64().unwrap());
    c.same("rational_cast<double>(a)", &F(a.to_f64()), &want);
}

#[test]
fn rationals_match() {
    replay("XRAT", xrat);
}

/// Both kinds are recorded (orca-arrange's `tests/probes.rs` checks that every kind of the file
/// has a replay).
#[test]
fn exact_kinds_are_recorded() {
    assert_eq!(orca_exact::NAME, "orca-exact");
    let file = file();
    for kind in EXACT_KINDS {
        assert!(
            file.of_kind(kind).next().is_some(),
            "{}: no {kind} record",
            file.path.display()
        );
    }
}
