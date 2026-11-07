// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries NLopt code: NLopt 2.5.0's src/algs/neldermead/sbplx.c and nldrmd.c, src/util/qsort_r.c,
// redblack.c and stop.c, src/api/optimize.c and options.c (the paths of NLOPT_LN_SBPLX and
// NLOPT_LN_NELDERMEAD, the elimination of fixed dimensions, the default initial step) and the
// optimize wrapper of src/api/nlopt-in.hpp, translated on 2026-11-07. Copyright (c) 2007-2014
// Massachusetts Institute of Technology, by Steven G. Johnson, under the MIT licence
// (licenses/MIT-NLopt.txt).

//! NLopt 2.5.0 as the C++ engine links it: the subplex method (`NLOPT_LN_SBPLX`, libnest2d's
//! `L_SUBPLEX`) over its Nelder-Mead subroutine, Nelder-Mead itself (`NLOPT_LN_NELDERMEAD`,
//! `L_SIMPLEX`), and the API path to them.
//!
//! - The build has no `qsort_r` (`nlopt_config.h`), so subplex sorts its index permutation with
//!   NLopt's own `nlopt_qsort_r`: an exchange sort below ten elements, else a quicksort on the
//!   median of three. Equal `|dx|` keep the order that algorithm leaves, so it is translated as
//!   written ([`qsort_r`]); no C library sort is involved.
//! - Nelder-Mead keeps its simplex in NLopt's red-black tree, keyed by the function value and,
//!   between equal values, by the row's address (`k1 - k2`), which is the row index here. The
//!   tree is translated node for node, so a NaN value, which makes that order inconsistent,
//!   lands where the C puts it.
//! - Clocks: `nlopt_optimize_` reads `nlopt_seconds()` (the build's `time(NULL)`) for
//!   `stop.start` and seeds NLopt's random generator from the time (`nlopt_srand_time_default`).
//!   Neither reaches a result here: the start time is read only for a `maxtime > 0`, which no
//!   caller sets (libnest2d's stop criteria have no time limit), and neither algorithm draws a
//!   random number. `parity/HAZARDS.md` lists both.
//! - Forced stops: the C++ wrapper's objective may call `force_stop()` on the `nlopt::opt` it was
//!   given; the flag is shared through [`Opt::force_stop_flag`]. When dimensions with `lb == ub`
//!   are eliminated, the algorithm runs on a copy with a flag of its own, so such a stop is not
//!   seen, as in the C.

use orca_crt::{copysign, fabs};
use orca_fmt::printf::{Arg, sprintf_string};
use std::cell::Cell;
use std::rc::Rc;

/// `HUGE_VAL`.
pub const HUGE_VAL: f64 = f64::INFINITY;

/// `nlopt_result`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NloptResult {
    /// `NLOPT_FAILURE`.
    Failure,
    /// `NLOPT_INVALID_ARGS`.
    InvalidArgs,
    /// `NLOPT_OUT_OF_MEMORY`.
    OutOfMemory,
    /// `NLOPT_ROUNDOFF_LIMITED`.
    RoundoffLimited,
    /// `NLOPT_FORCED_STOP`.
    ForcedStop,
    /// `NLOPT_SUCCESS`.
    Success,
    /// `NLOPT_STOPVAL_REACHED` (`NLOPT_MINF_MAX_REACHED`).
    StopvalReached,
    /// `NLOPT_FTOL_REACHED`.
    FtolReached,
    /// `NLOPT_XTOL_REACHED`.
    XtolReached,
    /// `NLOPT_MAXEVAL_REACHED`.
    MaxevalReached,
    /// `NLOPT_MAXTIME_REACHED`.
    MaxtimeReached,
}

impl NloptResult {
    /// The C enumerator's value.
    pub fn code(self) -> i32 {
        match self {
            NloptResult::Failure => -1,
            NloptResult::InvalidArgs => -2,
            NloptResult::OutOfMemory => -3,
            NloptResult::RoundoffLimited => -4,
            NloptResult::ForcedStop => -5,
            NloptResult::Success => 1,
            NloptResult::StopvalReached => 2,
            NloptResult::FtolReached => 3,
            NloptResult::XtolReached => 4,
            NloptResult::MaxevalReached => 5,
            NloptResult::MaxtimeReached => 6,
        }
    }
}

/// The two algorithms libnest2d maps its local methods to (`method2nloptAlg`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Algorithm {
    /// `NLOPT_LN_NELDERMEAD`.
    LnNelderMead,
    /// `NLOPT_LN_SBPLX`.
    LnSbplx,
}

impl Algorithm {
    /// The `nlopt_algorithm` enumerator's value.
    pub fn code(self) -> i32 {
        match self {
            Algorithm::LnNelderMead => 28,
            Algorithm::LnSbplx => 29,
        }
    }
}

/// What `nlopt::opt::optimize` throws (nlopt-in.hpp's `mythrow`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum NloptException {
    /// `std::runtime_error(get_errmsg() ? get_errmsg() : "nlopt failure")`.
    RuntimeError(String),
    /// `std::bad_alloc`.
    BadAlloc,
    /// `std::invalid_argument(get_errmsg() ? get_errmsg() : "nlopt invalid argument")`.
    InvalidArgument(String),
    /// `nlopt::roundoff_limited`.
    RoundoffLimited,
    /// `nlopt::forced_stop`.
    ForcedStop,
}

/// `nlopt_isinf`: `fabs(x) >= HUGE_VAL * 0.99`, or the NaN test of `x - x`.
#[allow(clippy::eq_op)] // x - x is NaN for an infinity, the C macro's own test
fn isinf(x: f64) -> bool {
    fabs(x) >= HUGE_VAL * 0.99 || (!x.is_nan() && (x - x).is_nan())
}

/// `nlopt_istiny`: zero or subnormal (`_fpclass`, the build has no `fpclassify`).
fn istiny(x: f64) -> bool {
    x == 0.0 || x.is_subnormal()
}

/// `struct nlopt_opt_s`, the parts the two algorithms use.
#[derive(Clone, Debug)]
pub struct Opt {
    algorithm: Algorithm,
    n: usize,
    lb: Vec<f64>,
    ub: Vec<f64>,
    maximize: bool,
    stopval: f64,
    ftol_rel: f64,
    ftol_abs: f64,
    xtol_rel: f64,
    xtol_abs: Vec<f64>,
    maxeval: i32,
    numevals: i32,
    maxtime: f64,
    force_stop: Rc<Cell<i32>>,
    dx: Option<Vec<f64>>,
    errmsg: Option<String>,
}

impl Opt {
    /// `nlopt_create(algorithm, n)`: bounds -inf..inf, no tolerances, `stopval = -HUGE_VAL`.
    pub fn create(algorithm: Algorithm, n: usize) -> Opt {
        let mut opt = Opt {
            algorithm,
            n,
            lb: vec![0.0; n],
            ub: vec![0.0; n],
            maximize: false,
            stopval: -HUGE_VAL,
            ftol_rel: 0.0,
            ftol_abs: 0.0,
            xtol_rel: 0.0,
            xtol_abs: vec![0.0; n],
            maxeval: 0,
            numevals: 0,
            maxtime: 0.0,
            force_stop: Rc::new(Cell::new(0)),
            dx: None,
            errmsg: None,
        };
        opt.set_lower_bounds1(-HUGE_VAL);
        opt.set_upper_bounds1(HUGE_VAL);
        opt.set_xtol_abs1(0.0);
        opt
    }

    /// `nlopt_copy`: a copy with a force-stop flag of its own (the C struct holds the flag).
    fn copy(&self) -> Opt {
        let mut c = self.clone();
        c.force_stop = Rc::new(Cell::new(self.force_stop.get()));
        c
    }

    /// `nlopt_get_dimension`.
    pub fn dimension(&self) -> usize {
        self.n
    }

    /// `nlopt_get_algorithm`.
    pub fn algorithm(&self) -> Algorithm {
        self.algorithm
    }

    /// `nlopt_get_numevals`.
    pub fn numevals(&self) -> i32 {
        self.numevals
    }

    /// The flag `nlopt_force_stop` sets, for an objective to share.
    pub fn force_stop_flag(&self) -> Rc<Cell<i32>> {
        Rc::clone(&self.force_stop)
    }

    /// `nlopt_force_stop`.
    pub fn force_stop(&self) {
        self.force_stop.set(1);
    }

    /// `nlopt_set_lower_bounds`: a bound within a subnormal of the upper one collapses onto it.
    pub fn set_lower_bounds(&mut self, lb: &[f64]) {
        self.errmsg = None;
        self.lb[..self.n].copy_from_slice(&lb[..self.n]);
        for i in 0..self.n {
            if self.lb[i] < self.ub[i] && istiny(self.ub[i] - self.lb[i]) {
                self.lb[i] = self.ub[i];
            }
        }
    }

    /// `nlopt_set_lower_bounds1`.
    pub fn set_lower_bounds1(&mut self, lb: f64) {
        self.errmsg = None;
        for i in 0..self.n {
            self.lb[i] = lb;
            if self.lb[i] < self.ub[i] && istiny(self.ub[i] - self.lb[i]) {
                self.lb[i] = self.ub[i];
            }
        }
    }

    /// `nlopt_set_upper_bounds`.
    pub fn set_upper_bounds(&mut self, ub: &[f64]) {
        self.errmsg = None;
        self.ub[..self.n].copy_from_slice(&ub[..self.n]);
        for i in 0..self.n {
            if self.lb[i] < self.ub[i] && istiny(self.ub[i] - self.lb[i]) {
                self.ub[i] = self.lb[i];
            }
        }
    }

    /// `nlopt_set_upper_bounds1`.
    pub fn set_upper_bounds1(&mut self, ub: f64) {
        self.errmsg = None;
        for i in 0..self.n {
            self.ub[i] = ub;
            if self.lb[i] < self.ub[i] && istiny(self.ub[i] - self.lb[i]) {
                self.ub[i] = self.lb[i];
            }
        }
    }

    /// `nlopt_set_stopval`.
    pub fn set_stopval(&mut self, v: f64) {
        self.errmsg = None;
        self.stopval = v;
    }

    /// `nlopt_set_ftol_rel`.
    pub fn set_ftol_rel(&mut self, v: f64) {
        self.errmsg = None;
        self.ftol_rel = v;
    }

    /// `nlopt_set_ftol_abs`.
    pub fn set_ftol_abs(&mut self, v: f64) {
        self.errmsg = None;
        self.ftol_abs = v;
    }

    /// `nlopt_set_xtol_rel`.
    pub fn set_xtol_rel(&mut self, v: f64) {
        self.errmsg = None;
        self.xtol_rel = v;
    }

    /// `nlopt_set_xtol_abs1`.
    pub fn set_xtol_abs1(&mut self, v: f64) {
        self.errmsg = None;
        for t in &mut self.xtol_abs {
            *t = v;
        }
    }

    /// `nlopt_set_maxeval`.
    pub fn set_maxeval(&mut self, v: i32) {
        self.errmsg = None;
        self.maxeval = v;
    }

    /// `nlopt_set_maxtime`.
    pub fn set_maxtime(&mut self, v: f64) {
        self.errmsg = None;
        self.maxtime = v;
    }

    /// `nlopt_set_min_objective`: also turns a `+HUGE_VAL` stopval (the maximizing default)
    /// back to `-HUGE_VAL`.
    pub fn set_min_objective(&mut self) {
        self.errmsg = None;
        self.maximize = false;
        if isinf(self.stopval) && self.stopval > 0.0 {
            self.stopval = -HUGE_VAL;
        }
    }

    /// `nlopt_set_max_objective`.
    pub fn set_max_objective(&mut self) {
        self.errmsg = None;
        self.maximize = true;
        if isinf(self.stopval) && self.stopval < 0.0 {
            self.stopval = HUGE_VAL;
        }
    }

    /// `nlopt_set_initial_step(opt, dx)`, `None` for the default (computed from `x` at each run).
    pub fn set_initial_step(&mut self, dx: Option<&[f64]>) -> NloptResult {
        self.errmsg = None;
        match dx {
            None => {
                self.dx = None;
                NloptResult::Success
            }
            Some(dx) => {
                for &d in &dx[..self.n] {
                    if d == 0.0 {
                        self.errmsg = Some("zero step size".to_string());
                        return NloptResult::InvalidArgs;
                    }
                }
                self.dx = Some(dx[..self.n].to_vec());
                NloptResult::Success
            }
        }
    }

    /// `nlopt_set_default_initial_step(opt, x)`: the crude step heuristics of options.c.
    fn set_default_initial_step(&mut self, x: &[f64]) {
        let mut dx = self.dx.take().unwrap_or_else(|| vec![1.0; self.n]);
        for i in 0..self.n {
            let (lb, ub) = (self.lb[i], self.ub[i]);
            let mut step = HUGE_VAL;
            if !isinf(ub) && !isinf(lb) && (ub - lb) * 0.25 < step && ub > lb {
                step = (ub - lb) * 0.25;
            }
            if !isinf(ub) && ub - x[i] < step && ub > x[i] {
                step = (ub - x[i]) * 0.75;
            }
            if !isinf(lb) && x[i] - lb < step && x[i] > lb {
                step = (x[i] - lb) * 0.75;
            }
            if isinf(step) {
                if !isinf(ub) && fabs(ub - x[i]) < fabs(step) {
                    step = (ub - x[i]) * 1.1;
                }
                if !isinf(lb) && fabs(x[i] - lb) < fabs(step) {
                    step = (x[i] - lb) * 1.1;
                }
            }
            if isinf(step) || istiny(step) {
                step = x[i];
            }
            if isinf(step) || step == 0.0 {
                step = 1.0;
            }
            dx[i] = step;
        }
        self.dx = Some(dx);
    }

    /// `nlopt_get_errmsg`.
    pub fn errmsg(&self) -> Option<&str> {
        self.errmsg.as_deref()
    }

    /// `nlopt_optimize(opt, x, opt_f)`: maximizing flips the signs, fixed dimensions are
    /// eliminated, and `nlopt_optimize_` runs the algorithm.
    pub fn optimize(
        &mut self,
        f: &mut dyn FnMut(&[f64]) -> f64,
        x: &mut [f64],
        opt_f: &mut f64,
    ) -> NloptResult {
        self.errmsg = None;
        let maximize = self.maximize;
        let mut flipped;
        let fun: &mut dyn FnMut(&[f64]) -> f64 = if maximize {
            flipped = |x: &[f64]| -> f64 { -f(x) };
            &mut flipped
        } else {
            f
        };
        if maximize {
            self.stopval = -self.stopval;
            self.maximize = false;
        }
        let ret = if self.elimdim_wrapcheck() {
            let fixed: Vec<bool> = (0..self.n).map(|i| self.lb[i] == self.ub[i]).collect();
            let mut elim = self.elimdim_create(&fixed);
            let n = self.n;
            let (lb, ub) = (self.lb.clone(), self.ub.clone());
            elimdim_shrink(x, &fixed);
            let mut full = vec![0.0; n];
            let mut elim_f = |x0: &[f64]| -> f64 {
                let mut j = 0;
                for i in 0..n {
                    if fixed[i] {
                        full[i] = lb[i];
                    } else {
                        full[i] = x0[j];
                        j += 1;
                    }
                }
                fun(&full)
            };
            // The copy's message stays with the copy: nlopt_get_errmsg of this one is NULL.
            let ret = elim.optimize_(&mut elim_f, x, opt_f);
            elimdim_expand(x, &fixed, &lb, &ub);
            ret
        } else {
            self.optimize_(fun, x, opt_f)
        };
        if maximize {
            self.maximize = true;
            self.stopval = -self.stopval;
            *opt_f = -*opt_f;
        }
        ret
    }

    /// `elimdim_wrapcheck`: whether some dimension has `lb == ub` (both algorithms qualify).
    fn elimdim_wrapcheck(&self) -> bool {
        (0..self.n).any(|i| self.lb[i] == self.ub[i])
    }

    /// `elimdim_create`: the copy over the free dimensions.
    fn elimdim_create(&self, fixed: &[bool]) -> Opt {
        let mut opt0 = self.copy();
        opt0.n = fixed.iter().filter(|&&f| !f).count();
        elimdim_shrink(&mut opt0.lb, fixed);
        elimdim_shrink(&mut opt0.ub, fixed);
        elimdim_shrink(&mut opt0.xtol_abs, fixed);
        if let Some(dx) = &mut opt0.dx {
            elimdim_shrink(dx, fixed);
        }
        opt0
    }

    /// `nlopt_optimize_`: the minimizing run.
    fn optimize_(
        &mut self,
        f: &mut dyn FnMut(&[f64]) -> f64,
        x: &mut [f64],
        minf: &mut f64,
    ) -> NloptResult {
        self.force_stop.set(0);
        let n = self.n;
        if n == 0 {
            *minf = f(&x[..0]);
            return NloptResult::Success;
        }
        *minf = HUGE_VAL;
        #[allow(clippy::needless_range_loop)] // the index is printed, as nlopt_optimize_ does
        for i in 0..n {
            if self.lb[i] > self.ub[i] || x[i] < self.lb[i] || x[i] > self.ub[i] {
                self.errmsg = sprintf_string(
                    "bounds %d fail %g <= %g <= %g",
                    &[
                        Arg::Int(i as i64),
                        Arg::Double(self.lb[i]),
                        Arg::Double(x[i]),
                        Arg::Double(self.ub[i]),
                    ],
                )
                .ok();
                return NloptResult::InvalidArgs;
            }
        }
        self.numevals = 0;
        let freedx = self.dx.is_none();
        if freedx {
            self.set_default_initial_step(x);
        }
        let dx = self.dx.clone().unwrap_or_default();
        let force_stop = Rc::clone(&self.force_stop);
        let mut stop = Stopping {
            n,
            minf_max: self.stopval,
            ftol_rel: self.ftol_rel,
            ftol_abs: self.ftol_abs,
            xtol_rel: self.xtol_rel,
            xtol_abs: self.xtol_abs.clone(),
            nevals: 0,
            maxeval: self.maxeval,
            maxtime: self.maxtime,
            start: if self.maxtime > 0.0 {
                Some(nlopt_seconds())
            } else {
                None
            },
            force_stop: &force_stop,
            stop_msg: None,
        };
        let (lb, ub) = (self.lb.clone(), self.ub.clone());
        let ret = match self.algorithm {
            Algorithm::LnNelderMead => nldrmd_minimize(n, f, &lb, &ub, x, minf, &dx, &mut stop),
            Algorithm::LnSbplx => sbplx_minimize(n, f, &lb, &ub, x, minf, &dx, &mut stop),
        };
        self.numevals = stop.nevals;
        if let Some(msg) = stop.stop_msg.take() {
            self.errmsg = Some(msg);
        }
        if freedx {
            self.dx = None;
        }
        ret
    }
}

/// `elimdim_shrink`: the free entries moved to the front.
fn elimdim_shrink(v: &mut [f64], fixed: &[bool]) {
    let mut j = 0;
    for i in 0..fixed.len() {
        if !fixed[i] {
            v[j] = v[i];
            j += 1;
        }
    }
}

/// `elimdim_expand`: the inverse of [`elimdim_shrink`], fixed entries set to their bound.
fn elimdim_expand(v: &mut [f64], fixed: &[bool], lb: &[f64], _ub: &[f64]) {
    let n = fixed.len();
    if n == 0 {
        return;
    }
    let mut j = fixed.iter().filter(|&&f| !f).count() as isize - 1;
    for i in (1..n).rev() {
        if !fixed[i] {
            v[i] = v[j as usize];
            j -= 1;
        } else {
            v[i] = lb[i];
        }
    }
    if fixed[0] {
        v[0] = lb[0];
    }
}

/// `nlopt::opt::optimize(x, opt_f)` of nlopt-in.hpp: the C result, or the exception the wrapper
/// throws for a negative one (`forced_stop` for a forced stop: the objective here throws
/// nothing, so the reason stays `NLOPT_FORCED_STOP`).
pub fn cxx_optimize(
    opt: &mut Opt,
    f: &mut dyn FnMut(&[f64]) -> f64,
    x: &mut [f64],
    opt_f: &mut f64,
) -> Result<NloptResult, NloptException> {
    if opt.dimension() != x.len() {
        return Err(NloptException::InvalidArgument(
            "dimension mismatch".to_string(),
        ));
    }
    let ret = if x.is_empty() {
        opt.errmsg = Some("NULL args to nlopt_optimize_".to_string());
        NloptResult::InvalidArgs
    } else {
        opt.optimize(f, x, opt_f)
    };
    match ret {
        NloptResult::Failure => Err(NloptException::RuntimeError(
            opt.errmsg().unwrap_or("nlopt failure").to_string(),
        )),
        NloptResult::OutOfMemory => Err(NloptException::BadAlloc),
        NloptResult::InvalidArgs => Err(NloptException::InvalidArgument(
            opt.errmsg().unwrap_or("nlopt invalid argument").to_string(),
        )),
        NloptResult::RoundoffLimited => Err(NloptException::RoundoffLimited),
        NloptResult::ForcedStop => Err(NloptException::ForcedStop),
        r => Ok(r),
    }
}

/// `nlopt_stopping`.
struct Stopping<'a> {
    n: usize,
    minf_max: f64,
    ftol_rel: f64,
    ftol_abs: f64,
    xtol_rel: f64,
    xtol_abs: Vec<f64>,
    nevals: i32,
    maxeval: i32,
    maxtime: f64,
    /// `stop.start`, read only when a time limit is set (the C reads it always and uses it for
    /// nothing else).
    start: Option<f64>,
    force_stop: &'a Cell<i32>,
    stop_msg: Option<String>,
}

/// `nlopt_seconds()` as the build has it (`HAVE_TIME`): `time(NULL)`.
fn nlopt_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as f64)
        .unwrap_or(0.0)
}

impl Stopping<'_> {
    /// `relstop(vold, vnew, reltol, abstol)`.
    fn relstop(vold: f64, vnew: f64, reltol: f64, abstol: f64) -> bool {
        if isinf(vold) {
            return false;
        }
        fabs(vnew - vold) < abstol
            || fabs(vnew - vold) < reltol * (fabs(vnew) + fabs(vold)) * 0.5
            || (reltol > 0.0 && vnew == vold)
    }

    /// `nlopt_stop_ftol(s, f, oldf)`.
    fn ftol(&self, f: f64, oldf: f64) -> bool {
        Stopping::relstop(oldf, f, self.ftol_rel, self.ftol_abs)
    }

    /// `nlopt_stop_x(s, x, oldx)`.
    fn x(&self, x: &[f64], oldx: &[f64]) -> bool {
        (0..self.n).all(|i| Stopping::relstop(oldx[i], x[i], self.xtol_rel, self.xtol_abs[i]))
    }

    /// `nlopt_stop_evals`.
    fn evals(&self) -> bool {
        self.maxeval > 0 && self.nevals >= self.maxeval
    }

    /// `nlopt_stop_time`: never true without a time limit.
    fn time(&self) -> bool {
        match self.start {
            Some(start) if self.maxtime > 0.0 => nlopt_seconds() - start >= self.maxtime,
            _ => false,
        }
    }

    /// `nlopt_stop_forced`.
    fn forced(&self) -> bool {
        self.force_stop.get() != 0
    }
}

/// `nlopt_qsort_r` of util/qsort_r.c (the build has no `qsort_r`).
pub fn qsort_r<T: Copy>(base: &mut [T], compar: &mut dyn FnMut(&T, &T) -> i32) {
    let nmemb = base.len();
    if nmemb < 10 {
        let mut i = 0;
        while i + 1 < nmemb {
            for j in i + 1..nmemb {
                if compar(&base[i], &base[j]) > 0 {
                    base.swap(i, j);
                }
            }
            i += 1;
        }
    } else {
        let (a, b, c) = (0, nmemb / 2, nmemb - 1);
        let mut pivot = if compar(&base[a], &base[b]) < 0 {
            if compar(&base[b], &base[c]) < 0 {
                nmemb / 2
            } else if compar(&base[a], &base[c]) < 0 {
                nmemb - 1
            } else {
                0
            }
        } else if compar(&base[a], &base[c]) < 0 {
            0
        } else if compar(&base[b], &base[c]) < 0 {
            nmemb - 1
        } else {
            nmemb / 2
        };
        base.swap(pivot, nmemb - 1);
        pivot = nmemb - 1;
        let mut npart = 0;
        for i in 0..nmemb - 1 {
            if compar(&base[i], &base[pivot]) <= 0 {
                base.swap(i, npart);
                npart += 1;
            }
        }
        base.swap(npart, pivot);
        qsort_r(&mut base[..npart], compar);
        npart += 1;
        qsort_r(&mut base[npart..], compar);
    }
}

/// `rb_color`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Color {
    Red,
    Black,
}

/// `rb_node`: parent, right, left, key (a row of the simplex) and colour.
#[derive(Clone, Copy, Debug)]
struct RbNode {
    p: usize,
    r: usize,
    l: usize,
    k: usize,
    c: Color,
}

/// The node standing for NULL (`nil`).
const NIL: usize = 0;

/// `rb_tree` of util/redblack.c over the rows of a simplex, compared by `simplex_compare`: the
/// value `pts[row * stride]` first, then the row (the C compares the rows' addresses).
struct RbTree {
    nodes: Vec<RbNode>,
    root: usize,
    stride: usize,
}

impl RbTree {
    /// `rb_tree_init`.
    fn new(stride: usize) -> RbTree {
        RbTree {
            nodes: vec![RbNode {
                p: NIL,
                r: NIL,
                l: NIL,
                k: 0,
                c: Color::Black,
            }],
            root: NIL,
            stride,
        }
    }

    /// `rb_tree_destroy` and `rb_tree_init` again.
    fn reset(&mut self) {
        self.nodes.truncate(1);
        self.root = NIL;
    }

    /// `simplex_compare(k1, k2)`: only its sign is used.
    fn compare(&self, pts: &[f64], k1: usize, k2: usize) -> i64 {
        let (v1, v2) = (pts[k1 * self.stride], pts[k2 * self.stride]);
        if v1 < v2 {
            return -1;
        }
        if v1 > v2 {
            return 1;
        }
        k1 as i64 - k2 as i64
    }

    fn rotate_left(&mut self, p: usize) {
        let n = self.nodes[p].r;
        self.nodes[p].r = self.nodes[n].l;
        self.nodes[n].l = p;
        let pp = self.nodes[p].p;
        if pp != NIL {
            if p == self.nodes[pp].l {
                self.nodes[pp].l = n;
            } else {
                self.nodes[pp].r = n;
            }
        } else {
            self.root = n;
        }
        self.nodes[n].p = pp;
        self.nodes[p].p = n;
        let pr = self.nodes[p].r;
        if pr != NIL {
            self.nodes[pr].p = p;
        }
    }

    fn rotate_right(&mut self, p: usize) {
        let n = self.nodes[p].l;
        self.nodes[p].l = self.nodes[n].r;
        self.nodes[n].r = p;
        let pp = self.nodes[p].p;
        if pp != NIL {
            if p == self.nodes[pp].l {
                self.nodes[pp].l = n;
            } else {
                self.nodes[pp].r = n;
            }
        } else {
            self.root = n;
        }
        self.nodes[n].p = pp;
        self.nodes[p].p = n;
        let pl = self.nodes[p].l;
        if pl != NIL {
            self.nodes[pl].p = p;
        }
    }

    /// `insert_node`.
    fn insert_node(&mut self, pts: &[f64], mut n: usize) {
        let k = self.nodes[n].k;
        let mut p = self.root;
        self.nodes[n].c = Color::Red;
        self.nodes[n].p = NIL;
        self.nodes[n].l = NIL;
        self.nodes[n].r = NIL;
        if p == NIL {
            self.root = n;
            self.nodes[n].c = Color::Black;
            return;
        }
        loop {
            if self.compare(pts, k, self.nodes[p].k) <= 0 {
                if self.nodes[p].l != NIL {
                    p = self.nodes[p].l;
                } else {
                    self.nodes[p].l = n;
                    self.nodes[n].p = p;
                    break;
                }
            } else if self.nodes[p].r != NIL {
                p = self.nodes[p].r;
            } else {
                self.nodes[p].r = n;
                self.nodes[n].p = p;
                break;
            }
        }
        loop {
            let np = self.nodes[n].p;
            if self.nodes[np].c != Color::Red {
                break;
            }
            let pp = self.nodes[p].p;
            let u = if p == self.nodes[pp].l {
                self.nodes[pp].r
            } else {
                self.nodes[pp].l
            };
            if u != NIL && self.nodes[u].c == Color::Red {
                self.nodes[p].c = Color::Black;
                self.nodes[u].c = Color::Black;
                n = pp;
                p = self.nodes[n].p;
                if p != NIL {
                    self.nodes[n].c = Color::Red;
                    continue;
                }
                break;
            }
            if n == self.nodes[p].r && p == self.nodes[pp].l {
                self.rotate_left(p);
                p = n;
                n = self.nodes[n].l;
            } else if n == self.nodes[p].l && p == self.nodes[pp].r {
                self.rotate_right(p);
                p = n;
                n = self.nodes[n].r;
            }
            self.nodes[p].c = Color::Black;
            let pp = self.nodes[p].p;
            self.nodes[pp].c = Color::Red;
            if n == self.nodes[p].l && p == self.nodes[pp].l {
                self.rotate_right(pp);
            } else if n == self.nodes[p].r && p == self.nodes[pp].r {
                self.rotate_left(pp);
            }
            break;
        }
    }

    /// `rb_tree_insert`.
    fn insert(&mut self, pts: &[f64], k: usize) -> usize {
        self.nodes.push(RbNode {
            p: NIL,
            r: NIL,
            l: NIL,
            k,
            c: Color::Red,
        });
        let n = self.nodes.len() - 1;
        self.insert_node(pts, n);
        n
    }

    /// `rb_tree_min`.
    fn min(&self) -> usize {
        let mut n = self.root;
        while n != NIL && self.nodes[n].l != NIL {
            n = self.nodes[n].l;
        }
        n
    }

    /// `rb_tree_max`.
    fn max(&self) -> usize {
        let mut n = self.root;
        while n != NIL && self.nodes[n].r != NIL {
            n = self.nodes[n].r;
        }
        n
    }

    /// `rb_tree_pred` (NIL for NULL).
    fn pred(&self, mut n: usize) -> usize {
        if n == NIL {
            return NIL;
        }
        if self.nodes[n].l == NIL {
            loop {
                let prev = n;
                n = self.nodes[n].p;
                if !(prev == self.nodes[n].l && n != NIL) {
                    break;
                }
            }
            n
        } else {
            n = self.nodes[n].l;
            while self.nodes[n].r != NIL {
                n = self.nodes[n].r;
            }
            n
        }
    }

    /// `rb_tree_remove`: returns the node taken out, which holds the removed key.
    fn remove(&mut self, mut n: usize) -> usize {
        let k = self.nodes[n].k;
        if self.nodes[n].l != NIL && self.nodes[n].r != NIL {
            let mut lmax = self.nodes[n].l;
            while self.nodes[lmax].r != NIL {
                lmax = self.nodes[lmax].r;
            }
            self.nodes[n].k = self.nodes[lmax].k;
            n = lmax;
        }
        let mut m = if self.nodes[n].l != NIL {
            self.nodes[n].l
        } else {
            self.nodes[n].r
        };
        let np = self.nodes[n].p;
        if np != NIL {
            if self.nodes[np].r == n {
                self.nodes[np].r = m;
            } else {
                self.nodes[np].l = m;
            }
        } else {
            self.root = m;
        }
        let mut mp = np;
        if m != NIL {
            self.nodes[m].p = mp;
        }
        if self.nodes[n].c == Color::Black {
            if self.nodes[m].c == Color::Red {
                self.nodes[m].c = Color::Black;
            } else {
                while mp != NIL {
                    let mut s = if m == self.nodes[mp].l {
                        self.nodes[mp].r
                    } else {
                        self.nodes[mp].l
                    };
                    if self.nodes[s].c == Color::Red {
                        self.nodes[mp].c = Color::Red;
                        self.nodes[s].c = Color::Black;
                        if m == self.nodes[mp].l {
                            self.rotate_left(mp);
                        } else {
                            self.rotate_right(mp);
                        }
                        s = if m == self.nodes[mp].l {
                            self.nodes[mp].r
                        } else {
                            self.nodes[mp].l
                        };
                    }
                    let (sl, sr) = (self.nodes[s].l, self.nodes[s].r);
                    let s_black_kids =
                        self.nodes[sl].c == Color::Black && self.nodes[sr].c == Color::Black;
                    if self.nodes[mp].c == Color::Black
                        && self.nodes[s].c == Color::Black
                        && s_black_kids
                    {
                        if s != NIL {
                            self.nodes[s].c = Color::Red;
                        }
                        m = mp;
                        mp = self.nodes[m].p;
                        continue;
                    } else if self.nodes[mp].c == Color::Red
                        && self.nodes[s].c == Color::Black
                        && s_black_kids
                    {
                        if s != NIL {
                            self.nodes[s].c = Color::Red;
                        }
                        self.nodes[mp].c = Color::Black;
                    } else {
                        if m == self.nodes[mp].l
                            && self.nodes[s].c == Color::Black
                            && self.nodes[sl].c == Color::Red
                            && self.nodes[sr].c == Color::Black
                        {
                            self.nodes[s].c = Color::Red;
                            self.nodes[sl].c = Color::Black;
                            self.rotate_right(s);
                            s = if m == self.nodes[mp].l {
                                self.nodes[mp].r
                            } else {
                                self.nodes[mp].l
                            };
                        } else if m == self.nodes[mp].r
                            && self.nodes[s].c == Color::Black
                            && self.nodes[sr].c == Color::Red
                            && self.nodes[sl].c == Color::Black
                        {
                            self.nodes[s].c = Color::Red;
                            self.nodes[sr].c = Color::Black;
                            self.rotate_left(s);
                            s = if m == self.nodes[mp].l {
                                self.nodes[mp].r
                            } else {
                                self.nodes[mp].l
                            };
                        }
                        self.nodes[s].c = self.nodes[mp].c;
                        self.nodes[mp].c = Color::Black;
                        if m == self.nodes[mp].l {
                            let sr = self.nodes[s].r;
                            self.nodes[sr].c = Color::Black;
                            self.rotate_left(mp);
                        } else {
                            let sl = self.nodes[s].l;
                            self.nodes[sl].c = Color::Black;
                            self.rotate_right(mp);
                        }
                    }
                    break;
                }
            }
        }
        self.nodes[n].k = k;
        n
    }

    /// `rb_tree_resort`.
    fn resort(&mut self, pts: &[f64], n: usize) -> usize {
        let n = self.remove(n);
        self.insert_node(pts, n);
        n
    }
}

/// Nelder-Mead's strategy constants: reflection, contraction, expansion and shrink.
const ALPHA: f64 = 1.0;
const BETA: f64 = 0.5;
const GAMM: f64 = 2.0;
const DELTA: f64 = 0.5;

/// `close(a, b)`: equal up to 1e-13 relative.
fn close(a: f64, b: f64) -> bool {
    fabs(a - b) <= 1e-13 * (fabs(a) + fabs(b))
}

/// `reflectpt`: `xnew = c + scale * (c - xold)` pinned to the bounds, false when it coincides
/// with `c` or `xold`. `xnew` and `xold` may be the same row (`Same`).
fn reflectpt(
    n: usize,
    xnew: &mut [f64],
    c: &[f64],
    scale: f64,
    xold: Option<&[f64]>,
    lb: &[f64],
    ub: &[f64],
) -> bool {
    let (mut equalc, mut equalold) = (true, true);
    for i in 0..n {
        let old = match xold {
            Some(o) => o[i],
            None => xnew[i],
        };
        let mut newx = c[i] + scale * (c[i] - old);
        if newx < lb[i] {
            newx = lb[i];
        }
        if newx > ub[i] {
            newx = ub[i];
        }
        equalc = equalc && close(newx, c[i]);
        equalold = equalold && close(newx, old);
        xnew[i] = newx;
    }
    !(equalc || equalold)
}

/// How a `CHECK_EVAL` ends the run, if it does.
fn check_eval(
    stop: &mut Stopping<'_>,
    x: &mut [f64],
    xc: &[f64],
    fc: f64,
    minf: &mut f64,
) -> Option<NloptResult> {
    stop.nevals += 1;
    if stop.forced() {
        return Some(NloptResult::ForcedStop);
    }
    if fc <= *minf {
        *minf = fc;
        x[..xc.len()].copy_from_slice(xc);
        if *minf < stop.minf_max {
            return Some(NloptResult::StopvalReached);
        }
    }
    if stop.evals() {
        return Some(NloptResult::MaxevalReached);
    }
    if stop.time() {
        return Some(NloptResult::MaxtimeReached);
    }
    None
}

/// `nldrmd_minimize_`: Nelder-Mead with `*minf = f(x)` given; a `psi > 0` replaces the tolerance
/// tests by the shrinking of the simplex diameter by `psi`. `fdiff` is set to the spread of the
/// last simplex.
#[allow(clippy::too_many_arguments)]
fn nldrmd_minimize_(
    n: usize,
    f: &mut dyn FnMut(&[f64]) -> f64,
    lb: &[f64],
    ub: &[f64],
    x: &mut [f64],
    minf: &mut f64,
    xstep: &[f64],
    stop: &mut Stopping<'_>,
    psi: f64,
    fdiff: &mut f64,
) -> NloptResult {
    let w = n + 1;
    let mut pts = vec![0.0; w * w];
    let mut c = vec![0.0; n];
    let mut xcur = vec![0.0; n];
    let ninv = 1.0 / n as f64;
    let mut init_diam = 0.0;
    let mut t = RbTree::new(w);
    *fdiff = HUGE_VAL;

    pts[1..w].copy_from_slice(&x[..n]);
    pts[0] = *minf;
    if *minf < stop.minf_max {
        return NloptResult::StopvalReached;
    }
    for i in 0..n {
        let row = (i + 1) * w;
        pts[row + 1..row + w].copy_from_slice(&x[..n]);
        pts[row + 1 + i] += xstep[i];
        if pts[row + 1 + i] > ub[i] {
            if ub[i] - x[i] > fabs(xstep[i]) * 0.1 {
                pts[row + 1 + i] = ub[i];
            } else {
                pts[row + 1 + i] = x[i] - fabs(xstep[i]);
            }
        }
        if pts[row + 1 + i] < lb[i] {
            if x[i] - lb[i] > fabs(xstep[i]) * 0.1 {
                pts[row + 1 + i] = lb[i];
            } else {
                pts[row + 1 + i] = x[i] + fabs(xstep[i]);
                if pts[row + 1 + i] > ub[i] {
                    pts[row + 1 + i] = 0.5
                        * ((if ub[i] - x[i] > x[i] - lb[i] {
                            ub[i]
                        } else {
                            lb[i]
                        }) + x[i]);
                }
            }
        }
        if close(pts[row + 1 + i], x[i]) {
            stop.stop_msg = sprintf_string(
                "starting step size led to simplex that was too small in dimension %d: %g is too close to x[%d]=%g",
                &[Arg::Int(i as i64), Arg::Double(pts[row + 1 + i]), Arg::Int(i as i64), Arg::Double(x[i])],
            )
            .ok();
            return NloptResult::Failure;
        }
        pts[row] = f(&pts[row + 1..row + w]);
        let pt = pts[row + 1..row + w].to_vec();
        if let Some(r) = check_eval(stop, x, &pt, pts[row], minf) {
            return r;
        }
    }

    'restart: loop {
        for i in 0..w {
            t.insert(&pts, i);
        }
        loop {
            let low = t.min();
            let high = t.max();
            let (lrow, hrow) = (t.nodes[low].k, t.nodes[high].k);
            let fl = pts[lrow * w];
            let mut fh = pts[hrow * w];
            *fdiff = fh - fl;

            if init_diam == 0.0 {
                for i in 0..n {
                    init_diam += fabs(pts[lrow * w + 1 + i] - pts[hrow * w + 1 + i]);
                }
            }

            if psi <= 0.0 && stop.ftol(fl, fh) {
                return NloptResult::FtolReached;
            }

            for v in c.iter_mut() {
                *v = 0.0;
            }
            for i in 0..w {
                if i != hrow {
                    for j in 0..n {
                        c[j] += pts[i * w + 1 + j];
                    }
                }
            }
            for v in c.iter_mut() {
                *v *= ninv;
            }

            for v in xcur.iter_mut() {
                *v = 0.0;
            }
            for i in 0..w {
                for j in 0..n {
                    let dx = fabs(pts[i * w + 1 + j] - c[j]);
                    if dx > xcur[j] {
                        xcur[j] = dx;
                    }
                }
            }
            for i in 0..n {
                xcur[i] += c[i];
            }
            if psi > 0.0 {
                let mut diam = 0.0;
                for i in 0..n {
                    diam += fabs(pts[lrow * w + 1 + i] - pts[hrow * w + 1 + i]);
                }
                if diam < psi * init_diam {
                    return NloptResult::XtolReached;
                }
            } else if stop.x(&c, &xcur) {
                return NloptResult::XtolReached;
            }

            let xh: Vec<f64> = pts[hrow * w + 1..hrow * w + w].to_vec();
            if !reflectpt(n, &mut xcur, &c, ALPHA, Some(&xh), lb, ub) {
                return NloptResult::XtolReached;
            }
            let fr = f(&xcur);
            if let Some(r) = check_eval(stop, x, &xcur, fr, minf) {
                return r;
            }

            if fr < fl {
                if !reflectpt(
                    n,
                    &mut pts[hrow * w + 1..hrow * w + w],
                    &c,
                    GAMM,
                    None,
                    lb,
                    ub,
                ) {
                    return NloptResult::XtolReached;
                }
                fh = f(&pts[hrow * w + 1..hrow * w + w]);
                let xh_now = pts[hrow * w + 1..hrow * w + w].to_vec();
                if let Some(r) = check_eval(stop, x, &xh_now, fh, minf) {
                    return r;
                }
                if fh >= fr {
                    fh = fr;
                    pts[hrow * w + 1..hrow * w + w].copy_from_slice(&xcur);
                }
            } else if fr < pts[t.nodes[t.pred(high)].k * w] {
                pts[hrow * w + 1..hrow * w + w].copy_from_slice(&xcur);
                fh = fr;
            } else {
                let scale = if fh <= fr { -BETA } else { BETA };
                let xh_old = pts[hrow * w + 1..hrow * w + w].to_vec();
                if !reflectpt(n, &mut xcur, &c, scale, Some(&xh_old), lb, ub) {
                    return NloptResult::XtolReached;
                }
                let fc = f(&xcur);
                if let Some(r) = check_eval(stop, x, &xcur, fc, minf) {
                    return r;
                }
                if fc < fr && fc < fh {
                    pts[hrow * w + 1..hrow * w + w].copy_from_slice(&xcur);
                    fh = fc;
                } else {
                    t.reset();
                    let xl = pts[lrow * w + 1..lrow * w + w].to_vec();
                    for i in 0..w {
                        if i != lrow {
                            if !reflectpt(
                                n,
                                &mut pts[i * w + 1..i * w + w],
                                &xl,
                                -DELTA,
                                None,
                                lb,
                                ub,
                            ) {
                                return NloptResult::XtolReached;
                            }
                            pts[i * w] = f(&pts[i * w + 1..i * w + w]);
                            let pt = pts[i * w + 1..i * w + w].to_vec();
                            if let Some(r) = check_eval(stop, x, &pt, pts[i * w], minf) {
                                return r;
                            }
                        }
                    }
                    continue 'restart;
                }
            }

            pts[hrow * w] = fh;
            t.resort(&pts, high);
        }
    }
}

/// `nldrmd_minimize`: Nelder-Mead from scratch (`NLOPT_LN_NELDERMEAD`).
#[allow(clippy::too_many_arguments)]
fn nldrmd_minimize(
    n: usize,
    f: &mut dyn FnMut(&[f64]) -> f64,
    lb: &[f64],
    ub: &[f64],
    x: &mut [f64],
    minf: &mut f64,
    xstep: &[f64],
    stop: &mut Stopping<'_>,
) -> NloptResult {
    *minf = f(&x[..n]);
    stop.nevals += 1;
    if stop.forced() {
        return NloptResult::ForcedStop;
    }
    if *minf < stop.minf_max {
        return NloptResult::StopvalReached;
    }
    if stop.evals() {
        return NloptResult::MaxevalReached;
    }
    if stop.time() {
        return NloptResult::MaxtimeReached;
    }
    let mut fdiff = 0.0;
    nldrmd_minimize_(n, f, lb, ub, x, minf, xstep, stop, 0.0, &mut fdiff)
}

/// Subplex's strategy constants.
const PSI: f64 = 0.25;
const OMEGA: f64 = 0.1;
const NSMIN: usize = 2;
const NSMAX: usize = 5;

/// `sbplx_minimize`: Rowan's subplex method (`NLOPT_LN_SBPLX`).
#[allow(clippy::too_many_arguments)]
fn sbplx_minimize(
    n: usize,
    f: &mut dyn FnMut(&[f64]) -> f64,
    lb: &[f64],
    ub: &[f64],
    x: &mut [f64],
    minf: &mut f64,
    xstep0: &[f64],
    stop: &mut Stopping<'_>,
) -> NloptResult {
    *minf = f(&x[..n]);
    stop.nevals += 1;
    if stop.forced() {
        return NloptResult::ForcedStop;
    }
    if *minf < stop.minf_max {
        return NloptResult::StopvalReached;
    }
    if stop.evals() {
        return NloptResult::MaxevalReached;
    }
    if stop.time() {
        return NloptResult::MaxtimeReached;
    }

    let mut xstep = xstep0[..n].to_vec();
    let mut xprev = vec![0.0; n];
    let mut dx = vec![0.0; n];
    let mut xs = [0.0; NSMAX];
    let mut xsstep = [0.0; NSMAX];
    let mut lbs = [0.0; NSMAX];
    let mut ubs = [0.0; NSMAX];
    let mut p: Vec<i32> = vec![0; n];

    loop {
        let mut normi = 0.0;
        let mut normdx = 0.0;
        let mut nsubs = 0;
        let mut fdiff = 0.0;
        let mut fdiff_max = 0.0;
        let mut ret;

        xprev.copy_from_slice(&x[..n]);

        for (i, v) in p.iter_mut().enumerate() {
            *v = i as i32;
        }
        qsort_r(&mut p, &mut |&i, &j| {
            let (dxi, dxj) = (fabs(dx[i as usize]), fabs(dx[j as usize]));
            if dxi > dxj {
                -1
            } else if dxi < dxj {
                1
            } else {
                0
            }
        });

        for v in &dx {
            normdx += fabs(*v);
        }
        let mut i = 0;
        let mut ns;
        while i + NSMIN < n {
            let mut ns_goodness = -HUGE_VAL;
            let mut norm = normi;
            let nk = if i + NSMAX > n { n } else { i + NSMAX };
            for k in i..i + NSMIN - 1 {
                norm += fabs(dx[p[k] as usize]);
            }
            ns = NSMIN;
            for k in i + NSMIN - 1..nk {
                norm += fabs(dx[p[k] as usize]);
                if n - (k + 1) < NSMIN {
                    continue;
                }
                let goodness = if k + 1 < n {
                    norm / (k + 1) as f64 - (normdx - norm) / (n - (k + 1)) as f64
                } else {
                    normdx / n as f64
                };
                if goodness > ns_goodness {
                    ns_goodness = goodness;
                    ns = (k + 1) - i;
                }
            }
            for k in i..i + ns {
                normi += fabs(dx[p[k] as usize]);
            }
            for k in i..i + ns {
                let pk = p[k] as usize;
                xs[k - i] = x[pk];
                xsstep[k - i] = xstep[pk];
                lbs[k - i] = lb[pk];
                ubs[k - i] = ub[pk];
            }
            nsubs += 1;
            ret = {
                let is = i;
                let pp = &p;
                let xx = &mut *x;
                let mut sub = |xsv: &[f64]| -> f64 {
                    for (k, &v) in xsv.iter().enumerate() {
                        xx[pp[is + k] as usize] = v;
                    }
                    f(&xx[..n])
                };
                nldrmd_minimize_(
                    ns,
                    &mut sub,
                    &lbs[..ns],
                    &ubs[..ns],
                    &mut xs[..ns],
                    minf,
                    &xsstep[..ns],
                    stop,
                    PSI,
                    &mut fdiff,
                )
            };
            if fdiff > fdiff_max {
                fdiff_max = fdiff;
            }
            for k in i..i + ns {
                x[p[k] as usize] = xs[k - i];
            }
            if ret == NloptResult::Failure {
                return NloptResult::XtolReached;
            }
            if ret != NloptResult::XtolReached {
                return ret;
            }
            i += ns;
        }
        ns = n - i;
        let is = i;
        while i < n {
            let pi = p[i] as usize;
            xs[i - is] = x[pi];
            xsstep[i - is] = xstep[pi];
            lbs[i - is] = lb[pi];
            ubs[i - is] = ub[pi];
            i += 1;
        }
        nsubs += 1;
        ret = {
            let pp = &p;
            let xx = &mut *x;
            let mut sub = |xsv: &[f64]| -> f64 {
                for (k, &v) in xsv.iter().enumerate() {
                    xx[pp[is + k] as usize] = v;
                }
                f(&xx[..n])
            };
            nldrmd_minimize_(
                ns,
                &mut sub,
                &lbs[..ns],
                &ubs[..ns],
                &mut xs[..ns],
                minf,
                &xsstep[..ns],
                stop,
                PSI,
                &mut fdiff,
            )
        };
        if fdiff > fdiff_max {
            fdiff_max = fdiff;
        }
        for i in is..n {
            x[p[i] as usize] = xs[i - is];
        }
        if ret == NloptResult::Failure {
            return NloptResult::XtolReached;
        }
        if ret != NloptResult::XtolReached {
            return ret;
        }

        if stop.ftol(*minf, *minf + fdiff_max) {
            return NloptResult::FtolReached;
        }
        if stop.x(&x[..n], &xprev) {
            let mut j = 0;
            while j < n {
                if fabs(xstep[j]) * PSI > stop.xtol_abs[j]
                    && fabs(xstep[j]) * PSI > stop.xtol_rel * fabs(x[j])
                {
                    break;
                }
                j += 1;
            }
            if j == n {
                return NloptResult::XtolReached;
            }
        }

        for i in 0..n {
            dx[i] = x[i] - xprev[i];
        }

        let scale = if nsubs == 1 {
            PSI
        } else {
            let (mut stepnorm, mut dxnorm) = (0.0, 0.0);
            for i in 0..n {
                stepnorm += fabs(xstep[i]);
                dxnorm += fabs(dx[i]);
            }
            let mut scale = dxnorm / stepnorm;
            #[allow(clippy::manual_clamp)] // two tests, as sbplx.c writes them (NaN stays)
            if scale < OMEGA {
                scale = OMEGA;
            }
            if scale > 1.0 / OMEGA {
                scale = 1.0 / OMEGA;
            }
            scale
        };
        for i in 0..n {
            xstep[i] = if dx[i] == 0.0 {
                -(xstep[i] * scale)
            } else {
                copysign(xstep[i] * scale, dx[i])
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qsort_r_sorts_both_ways() {
        let mut small = [5, 1, 4, 2, 3];
        qsort_r(&mut small, &mut |a, b| a - b);
        assert_eq!(small, [1, 2, 3, 4, 5]);
        let mut big: Vec<i32> = (0..40).map(|i| (i * 17) % 23).collect();
        qsort_r(&mut big, &mut |a, b| b - a);
        let mut want = big.clone();
        orca_stl::sort_by(&mut want, |a, b| a > b);
        assert_eq!(big, want);
    }

    #[test]
    fn subplex_finds_the_minimum_of_a_bowl() {
        let mut opt = Opt::create(Algorithm::LnSbplx, 2);
        opt.set_lower_bounds(&[-5.0, -5.0]);
        opt.set_upper_bounds(&[5.0, 5.0]);
        opt.set_ftol_rel(1e-12);
        opt.set_min_objective();
        let mut x = [3.0, -2.0];
        let mut fx = 0.0;
        let mut f = |x: &[f64]| (x[0] - 1.0) * (x[0] - 1.0) + (x[1] + 0.5) * (x[1] + 0.5);
        let r = cxx_optimize(&mut opt, &mut f, &mut x, &mut fx).unwrap();
        assert!(matches!(
            r,
            NloptResult::FtolReached | NloptResult::XtolReached
        ));
        assert!(fx < 1e-6, "{fx}");
        assert!(opt.numevals() > 10);
    }

    #[test]
    fn a_forced_stop_is_thrown_as_forced_stop() {
        let mut opt = Opt::create(Algorithm::LnSbplx, 1);
        opt.set_lower_bounds(&[0.0]);
        opt.set_upper_bounds(&[1.0]);
        let flag = opt.force_stop_flag();
        let mut calls = 0;
        let mut f = |x: &[f64]| {
            calls += 1;
            if calls == 3 {
                flag.set(1);
            }
            x[0]
        };
        let mut x = [0.5];
        let mut fx = 0.0;
        assert_eq!(
            cxx_optimize(&mut opt, &mut f, &mut x, &mut fx),
            Err(NloptException::ForcedStop)
        );
    }
}
