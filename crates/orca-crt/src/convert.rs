// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! MSVC 14.42's float-to-integer conversions: what a C++ cast `(int)x`, `(unsigned)x`,
//! `static_cast<uint64_t>(x)` and the rest give, for every integer width.
//!
//! Rust's `as` saturates and turns NaN into 0; the C++ engine's casts do not. MSVC 14.42 compiles
//! them (`/O2`, x64, no `/arch`, as the engine is built) to these instruction sequences, which
//! `cl.exe` was asked for and which the conversions here follow step by step:
//!
//! | C++ cast from `double` (`float`)     | MSVC 14.42 x64                                         |
//! | ------------------------------------ | ------------------------------------------------------ |
//! | `int8_t`, `uint8_t`, `int16_t`, `uint16_t`, `int32_t` | `cvttsd2si eax` (`cvttss2si eax`), then the low bits |
//! | `uint32_t`                           | `cvttsd2si rax`, then the low 32 bits                  |
//! | `int64_t`                            | `cvttsd2si rax`                                        |
//! | `uint64_t`                           | below 2^63 (or NaN): `cvttsd2si rax`; else subtract 2^63, `cvttsd2si rax` and add 2^63 back, unless the difference is still 2^63 or more |
//!
//! `cvttsd2si` truncates toward zero, and gives its "integer indefinite" value (the lowest value
//! of its width: `0x8000_0000` or `0x8000_0000_0000_0000`) for NaN, for an infinity, and for a
//! result that does not fit its 32 or 64 bits. So `(int16_t)70000.0` is 4464 (70000's low 16
//! bits), `(uint8_t)1e10` is 0 (the low byte of `0x8000_0000`), `(uint32_t)-1.0` is 0xFFFF_FFFF,
//! `(uint64_t)NaN` is 2^63 and `(uint64_t)1e30` is 2^63. The vectorised loops MSVC makes of the
//! same casts (`cvttpd2dq`, unrolled `cvttsd2si`) give the same values; `tests/crt.rs` holds the
//! values MSVC 14.42's own code gave for the edge cases.
//!
//! The other direction needs nothing of its own: MSVC 14.42 converts every integer type to
//! `double` and `float` correctly rounded (`cvtsi2sd`, and for `uint64_t` a halving with the low
//! bit kept, then a doubling), which is what Rust's `as` does.

/// `cvttsd2si r32`: truncation toward zero, and `0x8000_0000` for NaN or a result outside `i32`.
#[inline]
const fn cvtt_i32(x: f64) -> i32 {
    // Comparisons with NaN are false, so NaN takes the indefinite value. Every double strictly
    // between these bounds truncates into i32, and `as` truncates toward zero.
    if x > -2_147_483_649.0 && x < 2_147_483_648.0 {
        x as i32
    } else {
        i32::MIN
    }
}

/// `cvttsd2si r64`: truncation toward zero, and `0x8000_0000_0000_0000` for NaN or a result
/// outside `i64`.
#[inline]
const fn cvtt_i64(x: f64) -> i64 {
    // -2^63 is the lowest double that truncates into i64 (the next lower double is -2^63 - 2048).
    if x >= -9_223_372_036_854_775_808.0 && x < 9_223_372_036_854_775_808.0 {
        x as i64
    } else {
        i64::MIN
    }
}

/// `cvttss2si r32`, on a `float`.
#[inline]
const fn cvtt_i32_f32(x: f32) -> i32 {
    // -2^31 is the lowest float that truncates into i32 (the next lower float is -2^31 - 256).
    if x >= -2_147_483_648.0 && x < 2_147_483_648.0 {
        x as i32
    } else {
        i32::MIN
    }
}

/// `cvttss2si r64`, on a `float`.
#[inline]
const fn cvtt_i64_f32(x: f32) -> i64 {
    if x >= -9_223_372_036_854_775_808.0 && x < 9_223_372_036_854_775_808.0 {
        x as i64
    } else {
        i64::MIN
    }
}

/// `(int8_t)x` for a `double`: `cvttsd2si eax`, then the low 8 bits.
#[inline]
#[must_use]
pub const fn f64_to_i8(x: f64) -> i8 {
    cvtt_i32(x) as i8
}

/// `(uint8_t)x` for a `double`: `cvttsd2si eax`, then the low 8 bits.
#[inline]
#[must_use]
pub const fn f64_to_u8(x: f64) -> u8 {
    cvtt_i32(x) as u8
}

/// `(int16_t)x` for a `double`: `cvttsd2si eax`, then the low 16 bits.
#[inline]
#[must_use]
pub const fn f64_to_i16(x: f64) -> i16 {
    cvtt_i32(x) as i16
}

/// `(uint16_t)x` for a `double`: `cvttsd2si eax`, then the low 16 bits.
#[inline]
#[must_use]
pub const fn f64_to_u16(x: f64) -> u16 {
    cvtt_i32(x) as u16
}

/// `(int32_t)x` (`int`, `long`) for a `double`: `cvttsd2si eax`.
#[inline]
#[must_use]
pub const fn f64_to_i32(x: f64) -> i32 {
    cvtt_i32(x)
}

/// `(uint32_t)x` (`unsigned`, `unsigned long`) for a `double`: `cvttsd2si rax`, then the low 32
/// bits. So -1.0 gives 0xFFFF_FFFF, and NaN and anything from 2^63 up give 0.
#[inline]
#[must_use]
pub const fn f64_to_u32(x: f64) -> u32 {
    cvtt_i64(x) as u32
}

/// `(int64_t)x` (`long long`, `ptrdiff_t`) for a `double`: `cvttsd2si rax`.
#[inline]
#[must_use]
pub const fn f64_to_i64(x: f64) -> i64 {
    cvtt_i64(x)
}

/// `(uint64_t)x` (`unsigned long long`, `size_t`) for a `double`, MSVC's sequence:
///
/// ```text
/// movsd xmm1, 2^63 ; xor ecx, ecx
/// comisd xmm0, xmm1 ; jb L            ; x < 2^63, or NaN
/// subsd xmm0, xmm1 ; comisd xmm0, xmm1 ; jae L   ; x - 2^63 is still 2^63 or more
/// mov rcx, 0x8000000000000000
/// L: cvttsd2si rax, xmm0 ; add rax, rcx
/// ```
#[inline]
#[must_use]
pub const fn f64_to_u64(x: f64) -> u64 {
    const TWO_63: f64 = 9_223_372_036_854_775_808.0;
    // `jb` is taken for "below" and for unordered (NaN), so only `x >= 2^63` goes on.
    if x >= TWO_63 {
        let y = x - TWO_63;
        // `jae` is taken for "not below" and ordered: the difference does not fit either.
        if y >= TWO_63 {
            cvtt_i64(y) as u64
        } else {
            (cvtt_i64(y) as u64).wrapping_add(1 << 63)
        }
    } else {
        cvtt_i64(x) as u64
    }
}

/// `(int8_t)x` for a `float`: `cvttss2si eax`, then the low 8 bits.
#[inline]
#[must_use]
pub const fn f32_to_i8(x: f32) -> i8 {
    cvtt_i32_f32(x) as i8
}

/// `(uint8_t)x` for a `float`: `cvttss2si eax`, then the low 8 bits.
#[inline]
#[must_use]
pub const fn f32_to_u8(x: f32) -> u8 {
    cvtt_i32_f32(x) as u8
}

/// `(int16_t)x` for a `float`: `cvttss2si eax`, then the low 16 bits.
#[inline]
#[must_use]
pub const fn f32_to_i16(x: f32) -> i16 {
    cvtt_i32_f32(x) as i16
}

/// `(uint16_t)x` for a `float`: `cvttss2si eax`, then the low 16 bits.
#[inline]
#[must_use]
pub const fn f32_to_u16(x: f32) -> u16 {
    cvtt_i32_f32(x) as u16
}

/// `(int32_t)x` for a `float`: `cvttss2si eax`.
#[inline]
#[must_use]
pub const fn f32_to_i32(x: f32) -> i32 {
    cvtt_i32_f32(x)
}

/// `(uint32_t)x` for a `float`: `cvttss2si rax`, then the low 32 bits.
#[inline]
#[must_use]
pub const fn f32_to_u32(x: f32) -> u32 {
    cvtt_i64_f32(x) as u32
}

/// `(int64_t)x` for a `float`: `cvttss2si rax`.
#[inline]
#[must_use]
pub const fn f32_to_i64(x: f32) -> i64 {
    cvtt_i64_f32(x)
}

/// `(uint64_t)x` for a `float`: the sequence of [`f64_to_u64`] with `comiss`, `subss` and
/// `cvttss2si`.
#[inline]
#[must_use]
pub const fn f32_to_u64(x: f32) -> u64 {
    const TWO_63: f32 = 9_223_372_036_854_775_808.0;
    if x >= TWO_63 {
        let y = x - TWO_63;
        if y >= TWO_63 {
            cvtt_i64_f32(y) as u64
        } else {
            (cvtt_i64_f32(y) as u64).wrapping_add(1 << 63)
        }
    } else {
        cvtt_i64_f32(x) as u64
    }
}
