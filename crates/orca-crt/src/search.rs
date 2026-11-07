// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! `qsort` of the static UCRT (`corecrt_search.h`), called as the C++ engine's C code calls it.
//!
//! Qhull sorts with the C library's `qsort` (added 2026-11-07, for phase O2): `mem_r.c` its table
//! of memory sizes (`int`s, with `qh_intcompare`), `merge_r.c` arrays of pointers to merges and
//! vertices, `io_r.c` arrays of pointers to facets. Qhull's comparators do not all define an
//! order: `qh_compareangle` never returns 0, so equal angles come out in whatever order `qsort`'s
//! sequence of calls makes of them. That sequence is the UCRT's, whose source is not open, so
//! [`qsort`] calls the function itself, the one the program links statically
//! (`.cargo/config.toml`), on the elements where they are, with a comparator that calls the Rust
//! closure.
//!
//! What the UCRT's `qsort` does, found by black-box tests (`tests/crt.rs`) and held to the C++
//! engine's own build by the probe's QSRT records (`tests/probes.rs`):
//!
//! - it gives the comparator pointers to elements of the array itself, at whole multiples of the
//!   element size from its start, so [`qsort`] hands them on as references;
//! - it calls the comparator no time for fewer than two elements;
//! - its sequence of calls, and so the order it leaves equal elements in, depends on the values
//!   the comparator returns and not on the element size: an array of 4-byte ids sorts as the C++
//!   array of 8-byte pointers to the same objects does.
//!
//! NLopt's `nlopt_qsort_r` is not the CRT's: NLopt 2.5.0 always takes its own quicksort
//! (`util/qsort_r.c` calls the C library only under `HAVE_QSORT_R_damn_it_use_my_own`, which
//! nothing defines), so the C++ engine calls neither `qsort_s` nor `qsort_r`, and this crate has
//! neither. That quicksort is NLopt's code and is translated with NLopt.

use core::cell::Cell;
use core::ffi::c_void;

/// `qsort` of the static UCRT (`corecrt_search.h`, x64), as `libucrt.lib` exports it.
mod ucrt {
    use core::ffi::c_void;

    unsafe extern "C" {
        pub unsafe fn qsort(
            base: *mut c_void,
            count: usize,
            size: usize,
            compare: unsafe extern "C" fn(*const c_void, *const c_void) -> i32,
        );
    }
}

thread_local! {
    /// The closure of the innermost [`qsort`] running on this thread. C's `qsort` passes its
    /// comparator no context, so the comparator finds the closure here; a closure that sorts again
    /// sets its own and puts this one back afterwards.
    static COMPARE: Cell<*mut c_void> = const { Cell::new(core::ptr::null_mut()) };
}

/// `qsort(v, v.len(), sizeof(T), compare)` with the static UCRT's own `qsort`: sorts `v` in place.
///
/// `compare(a, b)` is the C comparator: negative when `a` goes before `b`, positive when after,
/// zero when either may. The UCRT calls it in its own sequence, which the translation of a C
/// caller needs exactly when the comparator ties or contradicts itself (Qhull's
/// `qh_compareangle`), so pass elements in the order the C++ array holds them.
///
/// # Panics
///
/// For a zero-sized `T`, which a C array cannot hold. A `compare` that panics ends the process:
/// the panic cannot unwind through the UCRT's frames.
pub fn qsort<T, F>(v: &mut [T], mut compare: F)
where
    F: FnMut(&T, &T) -> i32,
{
    assert!(size_of::<T>() != 0, "qsort of a zero-sized type");
    let previous = COMPARE.with(|c| c.replace((&raw mut compare).cast::<c_void>()));
    // SAFETY: `v` is `v.len()` initialised elements of `size_of::<T>()` bytes each, which `qsort`
    // only permutes (Rust values move by their bytes); `compare_trampoline::<T, F>` reads this
    // thread's COMPARE, which holds `compare`, an `F`, until the call returns.
    unsafe {
        ucrt::qsort(
            v.as_mut_ptr().cast::<c_void>(),
            v.len(),
            size_of::<T>(),
            compare_trampoline::<T, F>,
        );
    }
    COMPARE.with(|c| c.set(previous));
}

/// The comparator the UCRT calls: this thread's innermost closure, on the two elements.
unsafe extern "C" fn compare_trampoline<T, F>(a: *const c_void, b: *const c_void) -> i32
where
    F: FnMut(&T, &T) -> i32,
{
    let (a, b) = (a.cast::<T>(), b.cast::<T>());
    assert!(
        a.is_aligned() && b.is_aligned(),
        "the UCRT's qsort gave its comparator a misaligned element"
    );
    // SAFETY: COMPARE holds the `F` of the `qsort` call this comparator runs under: [`qsort`] sets
    // it before calling the UCRT and puts the previous one back after, on this thread, and only
    // `compare_trampoline::<T, F>` with the same `F` reads it during that call. `a` and `b` point
    // at elements of that call's array (`tests/crt.rs` checks the UCRT passes nothing else), which
    // stay where they are while the comparator runs.
    unsafe {
        let compare = &mut *COMPARE.with(Cell::get).cast::<F>();
        compare(&*a, &*b)
    }
}
