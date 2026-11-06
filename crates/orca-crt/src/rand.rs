// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! `rand` and `srand` as the UCRT has them: a linear congruential generator with one state per
//! thread.
//!
//! The C++ engine's `rand()` calls (Lightning's tree nodes, `Fill.cpp`'s joints) read and advance
//! the calling thread's own state, which starts at 1 on every thread, so a thread that never called
//! `srand` begins with the same sequence as every other. Each step multiplies the 32-bit state by
//! 214013 and adds 2531011 (wrapping), and `rand` returns bits 16 to 30 of the new state: a value
//! from 0 to [`RAND_MAX`] (32767). `tests/crt.rs` checks the sequences against the UCRT's own
//! `rand` and `srand` on fresh threads.
//!
//! The state is kept here, not in the UCRT, so that a long-running program can reset it for each
//! job ([`set_rand_state`]) the way a fresh C++ process starts with it.

use core::cell::Cell;

/// `RAND_MAX`: the largest value [`rand`] returns.
pub const RAND_MAX: i32 = 0x7FFF;

/// The state of a thread that has not called [`srand`], as in the UCRT.
pub const RAND_INITIAL_STATE: u32 = 1;

thread_local! {
    /// The calling thread's generator state (the UCRT keeps it in its per-thread data).
    static STATE: Cell<u32> = const { Cell::new(RAND_INITIAL_STATE) };
}

/// `srand(seed)`: sets the calling thread's state to `seed`.
#[inline]
pub fn srand(seed: u32) {
    STATE.with(|s| s.set(seed));
}

/// `rand()`: advances the calling thread's state and returns a value from 0 to [`RAND_MAX`].
#[inline]
pub fn rand() -> i32 {
    STATE.with(|s| {
        let next = s.get().wrapping_mul(214_013).wrapping_add(2_531_011);
        s.set(next);
        // Bits 16 to 30: at most 15 bits, so the conversion is exact.
        ((next >> 16) & 0x7FFF) as i32
    })
}

/// The calling thread's state, as [`srand`] or the last [`rand`] left it.
#[inline]
#[must_use]
pub fn rand_state() -> u32 {
    STATE.with(Cell::get)
}

/// Sets the calling thread's state: [`RAND_INITIAL_STATE`] makes the thread start over as a new
/// thread of a new C++ process would.
#[inline]
pub fn set_rand_state(state: u32) {
    STATE.with(|s| s.set(state));
}
