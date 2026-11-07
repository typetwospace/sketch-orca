// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Libnest2D code: libnest2d as OrcaSlicer v2.4.2 carries it in deps_src/libnest2d,
// translated on 2026-11-07 from include/libnest2d/optimizer.hpp (StopCriteria, Result,
// ResultCodes, optimize_min) and optimizers/nlopt/nlopt_boilerplate.hpp and subplex.hpp
// (NloptOptimizer::optimize, optfunc), with the objective trampoline of NLopt 2.5.0's
// src/api/nlopt-in.hpp (myfunc) under the MIT licence. Copyright (C) Tamas Meszaros, modified
// by Prusa Research, under the GNU Lesser General Public License, version 3
// (licenses/LGPL-3.0.txt).

//! libnest2d's `TOptimizer<L_SUBPLEX>`: NLopt's `LN_SBPLX` run through `nlopt::opt`.
//!
//! - Each call makes a fresh `nlopt::opt` with the bounds, sets the absolute and relative score
//!   tolerances and the stop value when they are not NaN, and the evaluation limit when it is
//!   above 0, then minimises from the initial values; the optimum is the final point.
//! - A forced stop is caught (`FORCED_STOP`); NLopt's other exceptions leave the call, as
//!   [`ArrangeError::Nlopt`].
//! - An objective that fails does what nlopt-in.hpp's `myfunc` does with an exception: records
//!   it as the stop reason, forces a stop and returns `HUGE_VAL`; the optimisation then ends
//!   with the exception of that reason (`nlopt::forced_stop`, which libnest2d catches, only when
//!   the objective itself was forced to stop).
//! - libnest2d's stop condition (`stopcr_.stop_condition`) is never set by `Arrange.cpp`, so it
//!   is not carried.

use crate::ArrangeError;
use crate::nlopt::{Algorithm, HUGE_VAL, NloptException, Opt, cxx_optimize};
use std::cell::RefCell;

/// `opt::StopCriteria`, without the stop condition.
#[derive(Clone, Copy, Debug)]
pub struct StopCriteria {
    /// `absolute_score_difference`: NaN for none.
    pub absolute_score_difference: f64,
    /// `relative_score_difference`: NaN for none.
    pub relative_score_difference: f64,
    /// `stop_score`: NaN for none.
    pub stop_score: f64,
    /// `max_iterations`: 0 for none.
    pub max_iterations: u32,
}

impl Default for StopCriteria {
    fn default() -> StopCriteria {
        StopCriteria {
            absolute_score_difference: f64::NAN,
            relative_score_difference: f64::NAN,
            stop_score: f64::NAN,
            max_iterations: 0,
        }
    }
}

/// `opt::Result<double...>`: the result code, the optimum and its score. The value-initialised
/// result (`results(n)` in the placer) is all zeros.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OptResult<const N: usize> {
    /// `resultcode`, as NLopt's code.
    pub resultcode: i32,
    /// `optimum`.
    pub optimum: [f64; N],
    /// `score`.
    pub score: f64,
}

impl<const N: usize> Default for OptResult<N> {
    fn default() -> OptResult<N> {
        OptResult {
            resultcode: 0,
            optimum: [0.0; N],
            score: 0.0,
        }
    }
}

/// `NLOPT_FORCED_STOP`.
const FORCED_STOP: i32 = -5;

/// The exception `optimize` throws again for an exception of the objective: `myfunc` keeps only
/// its kind as the stop reason, and `force_stop` clears NLopt's message, so `mythrow` throws the
/// kind's default (`std::invalid_argument("nlopt invalid argument")`, `std::runtime_error("nlopt
/// failure")` for any other kind).
fn reason_exception(e: &ArrangeError) -> NloptException {
    match e {
        ArrangeError::Nlopt(NloptException::BadAlloc) => NloptException::BadAlloc,
        ArrangeError::Nlopt(NloptException::InvalidArgument(_)) => {
            NloptException::InvalidArgument("nlopt invalid argument".to_string())
        }
        ArrangeError::Nlopt(NloptException::RoundoffLimited) => NloptException::RoundoffLimited,
        ArrangeError::Nlopt(NloptException::ForcedStop) => NloptException::ForcedStop,
        _ => NloptException::RuntimeError("nlopt failure".to_string()),
    }
}

/// `optimize_min(func, initvals, bounds...)` of an `L_SUBPLEX` optimizer over `N` variables.
///
/// # Errors
///
/// NLopt's exceptions other than a forced stop, and the exception an objective failure turns
/// into.
pub fn optimize_min<const N: usize>(
    stopcr: &StopCriteria,
    func: &mut dyn FnMut(&[f64]) -> Result<f64, ArrangeError>,
    initvals: [f64; N],
    bounds: [(f64, f64); N],
) -> Result<OptResult<N>, ArrangeError> {
    let lower: Vec<f64> = bounds.iter().map(|b| b.0).collect();
    let upper: Vec<f64> = bounds.iter().map(|b| b.1).collect();
    let mut opt = Opt::create(Algorithm::LnSbplx, N);
    opt.set_lower_bounds(&lower);
    opt.set_upper_bounds(&upper);
    if !stopcr.absolute_score_difference.is_nan() {
        opt.set_ftol_abs(stopcr.absolute_score_difference);
    }
    if !stopcr.relative_score_difference.is_nan() {
        opt.set_ftol_rel(stopcr.relative_score_difference);
    }
    if !stopcr.stop_score.is_nan() {
        opt.set_stopval(stopcr.stop_score);
    }
    if stopcr.max_iterations > 0 {
        opt.set_maxeval(stopcr.max_iterations as i32);
    }
    let mut x = initvals.to_vec();
    opt.set_min_objective();
    let flag = opt.force_stop_flag();
    let reason: RefCell<Option<NloptException>> = RefCell::new(None);
    let mut trampoline = |params: &[f64]| -> f64 {
        match func(params) {
            Ok(v) => v,
            Err(e) => {
                *reason.borrow_mut() = Some(reason_exception(&e));
                flag.set(1);
                HUGE_VAL
            }
        }
    };
    let mut score = 0.0;
    let ret = cxx_optimize(&mut opt, &mut trampoline, &mut x, &mut score);
    let resultcode = match ret {
        Ok(r) => r.code(),
        Err(NloptException::ForcedStop) => match reason.into_inner() {
            None | Some(NloptException::ForcedStop) => FORCED_STOP,
            Some(other) => return Err(ArrangeError::Nlopt(other)),
        },
        Err(e) => return Err(ArrangeError::Nlopt(e)),
    };
    let mut optimum = [0.0; N];
    optimum.copy_from_slice(&x);
    Ok(OptResult {
        resultcode,
        optimum,
        score,
    })
}

/// The placer's `Optimizer(accuracy)`: `max_iterations = unsigned(floor(1000 * accuracy))` in
/// `float`, relative score difference `1e-20`.
#[must_use]
pub fn placer_stop_criteria(accuracy: f32) -> StopCriteria {
    StopCriteria {
        max_iterations: orca_crt::f32_to_u32(orca_crt::floorf(1000.0f32 * accuracy)),
        relative_score_difference: 1e-20,
        ..StopCriteria::default()
    }
}
