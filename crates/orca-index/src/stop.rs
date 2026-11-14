// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! [`NotPorted`]: the stop of an input the C++ engine has no answer for (added by index-1,
//! 2026-11-13).
//!
//! OrcaSlicer's release build defines `NDEBUG`, so its asserts do not run, and on some inputs it
//! reads past a vector's end, divides by zero or overflows a signed product: undefined behaviour,
//! whose result nothing can copy. `parity/sop1/index.md` lists those inputs as IXNP rows, each
//! naming the piece, the functions and the condition. On such an input this crate does not guess:
//! it panics with a [`NotPorted`] payload ([`not_ported`]) whose `function` and `condition` are
//! the row's texts, byte for byte, which the probe test catches with `catch_unwind` and compares.
//! The engine never runs on these inputs, so no caller reaches the stop.

use core::fmt;

/// The payload of the panic [`not_ported`] raises: the functions of the IXNP row
/// (`parity/sop1/index.md`) and its condition, as the row writes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotPorted {
    /// The functions, as the row names them (`"MutablePriorityQueue::top, MutablePriorityQueue::pop"`).
    pub function: &'static str,
    /// The condition the input meets, as the row writes it.
    pub condition: &'static str,
}

impl fmt::Display for NotPorted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "not ported: {}: {} (the C++ engine has no answer here)",
            self.function, self.condition
        )
    }
}

/// Stops with a [`NotPorted`] payload: `std::panic::panic_any`, which `catch_unwind` and
/// `downcast::<NotPorted>()` catch.
pub fn not_ported(function: &'static str, condition: &'static str) -> ! {
    std::panic::panic_any(NotPorted {
        function,
        condition,
    })
}
