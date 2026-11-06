// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Microsoft STL code: from_chars_int is translated from stl/inc/charconv
// (_Integer_from_chars), from_chars_float and its helpers from stl/inc/charconv (_Big_integer_flt
// and its operations, _Assemble_floating_point_zero, _Assemble_floating_point_infinity,
// _Right_shift_with_rounding, _Assemble_floating_point_value_no_shift,
// _Assemble_floating_point_value, _Assemble_floating_point_value_from_big_integer_flt,
// _Accumulate_decimal_digits_into_big_integer_flt, _Convert_decimal_string_to_floating_type,
// _Convert_hexadecimal_string_to_floating_type, _Ordinary_floating_from_chars,
// _Infinity_from_chars, _Nan_from_chars and _Floating_from_chars) and stl/inc/xbit_ops.h
// (_Bit_scan_reverse), and stoi, stol, stoul, stoll, stoull, stof and stod from stl/inc/string, of
// microsoft/STL at the tag vs-2022-17.12 (commit 91e425587a0b9b5ac0d1005844c661fee2e2813b), the
// headers of MSVC 14.42, function for function, on 2026-11-06. Its copyright holder is Microsoft
// Corporation, and it is under Apache-2.0 WITH LLVM-exception (licenses/Apache-2.0.txt and
// licenses/LLVM-exception.txt). The STL marks its floating-point parser as derived from the UCRT's
// corecrt_internal_big_integer.h and corecrt_internal_strtox.h; Microsoft publishes it, in the STL,
// under that licence. strtod, strtof, the strtol family and atof, atoi, atol and atoll are not the
// UCRT's code, whose source is not under an open licence: they are written from the C standard (C17 7.22.1) around
// the STL's parser, with the UCRT's choices where the standard leaves them open and the three
// places where the UCRT's rounding differs from the STL's (Parser), and tests/fmt.rs compares them
// with the UCRT's own functions.

//! Number parsing as the C++ engine's runtime does it: `std::from_chars` of MSVC 14.42's standard
//! library, the UCRT's `strtod`, `strtof`, `strtol`, `strtoul`, `strtoll`, `strtoull`, `atof`,
//! `atoi`, `atol` and `atoll`, and the STL's `std::stod`, `stof`, `stoi`, `stol`, `stoul`, `stoll`
//! and `stoull` built on them.
//!
//! - [`from_chars_int`] and [`from_chars_float`]: `std::from_chars`. No whitespace and no `+`; a
//!   `-` for the signed types and the floating types; hexadecimal without `0x`. On
//!   `errc::invalid_argument` the value is left as it was; on `errc::result_out_of_range` an
//!   integer is left as it was and a floating value is set to the infinity or zero it rounds to,
//!   as MSVC does. The floating parse is exact: up to 768 significant digits are kept, and the
//!   rest only say whether the value lies above the 768-digit one, so every input rounds
//!   correctly, ties to even.
//! - [`strtod`] and [`strtof`]: leading white space (`isspace` in the "C" locale), a `+` or `-`,
//!   then a decimal number, `0x` and a hexadecimal one, `inf`, `infinity`, `nan` or `nan(...)`, in
//!   any case. The number is the STL's parser above with the UCRT's three differences: digits after
//!   the 768th dropped, an exact tie rounded up after some divisions, and the exponent capped at
//!   5201 (the private `Parser` enum says where). The UCRT's choices: `nan` and `nan(...)` give the
//!   quiet NaN with every payload bit set, `nan(ind)` the negative quiet NaN without payload
//!   (whatever its sign), `nan(snan)` the signalling NaN with payload 1, and a `(` that begins
//!   `snan)` and goes on `ind)` is `(ind)` too; `0x` without a hex digit reads as the `0`;
//!   `ERANGE` for an overflow (to an infinity) and for a result that rounds to zero, not for one
//!   that is merely subnormal.
//! - [`strtol`], [`strtoul`], [`strtoll`], [`strtoull`]: `long` is 32 bits. Base 0 reads `0x` as
//!   hexadecimal and a leading `0` as octal; with base 0 or 16, a `0x` without a hex digit after it
//!   is no number at all (the UCRT's choice); an overflow gives the limit and `ERANGE`; `strtoul`
//!   and `strtoull` negate a `-` number modulo 2^N. A base other than 0 and 2 to 36 is `EINVAL`
//!   (the UCRT's invalid parameter).
//! - [`stod`] and the other `sto*`: the STL's code, `std::invalid_argument` when nothing was read
//!   and `std::out_of_range` on `ERANGE`, as [`StoError`].
//!
//! Every input is a byte string; the C functions read it as a C string, up to its first NUL.

use crate::Errc;
use crate::charconv_tables::LARGE_POWER_DATA;
use crate::to_chars::CharsFormat;

/// `std::from_chars_result`: where the parse stopped, as an index into the input, and its error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FromCharsResult {
    /// `ptr - first`: the bytes read, or 0 for `errc::invalid_argument`.
    pub ptr: usize,
    /// `ec`: `None` for success (`errc{}`).
    pub ec: Option<Errc>,
}

mod private {
    /// The constants of an integer type that `from_chars` and `strto*` need.
    pub trait Integer: Copy {
        const BITS: u32;
        const SIGNED: bool;
        /// `static_cast<_RawTy>(_Value)`: the low `BITS` bits.
        fn from_raw(raw: u64) -> Self;
    }

    /// `_Floating_type_traits<_FloatingType>`.
    pub trait Floating: Copy {
        const MANTISSA_BITS: i32;
        const MAXIMUM_BINARY_EXPONENT: i32;
        const MINIMUM_BINARY_EXPONENT: i32;
        const EXPONENT_BIAS: i32;
        const SIGN_SHIFT: u32;
        const EXPONENT_SHIFT: u32;
        const NORMAL_MANTISSA_MASK: u64;
        const DENORMAL_MANTISSA_MASK: u64;
        const SPECIAL_NAN_MANTISSA_MASK: u64;
        const SHIFTED_SIGN_MASK: u64;
        const SHIFTED_EXPONENT_MASK: u64;
        /// `_Bit_cast<_FloatingType>` of the low bits.
        fn from_uint(bits: u64) -> Self;
        /// Zero, for a C function that converted nothing.
        fn zero() -> Self;
    }
}

/// An integer type that [`from_chars_int`] parses: `i8`, `u8`, `i16`, `u16`, `i32`, `u32`, `i64`,
/// `u64`, `isize` and `usize` (`char` is `i8` with MSVC).
pub trait FromCharsInteger: private::Integer {}

/// A floating type that [`from_chars_float`] parses: `f32` and `f64` (and `long double`, which is
/// `double` with MSVC).
pub trait FromCharsFloat: private::Floating {}

macro_rules! integer {
    ($($t:ty => $signed:expr),* $(,)?) => {
        $(
            impl private::Integer for $t {
                const BITS: u32 = <$t>::BITS;
                const SIGNED: bool = $signed;
                fn from_raw(raw: u64) -> Self {
                    raw as $t
                }
            }
            impl FromCharsInteger for $t {}
        )*
    };
}

integer!(
    i8 => true, u8 => false, i16 => true, u16 => false, i32 => true, u32 => false,
    i64 => true, u64 => false, isize => true, usize => false,
);

impl private::Floating for f32 {
    const MANTISSA_BITS: i32 = 24;
    const MAXIMUM_BINARY_EXPONENT: i32 = 127;
    const MINIMUM_BINARY_EXPONENT: i32 = -126;
    const EXPONENT_BIAS: i32 = 127;
    const SIGN_SHIFT: u32 = 31;
    const EXPONENT_SHIFT: u32 = 23;
    const NORMAL_MANTISSA_MASK: u64 = 0x00FF_FFFF;
    const DENORMAL_MANTISSA_MASK: u64 = 0x007F_FFFF;
    const SPECIAL_NAN_MANTISSA_MASK: u64 = 0x0040_0000;
    const SHIFTED_SIGN_MASK: u64 = 0x8000_0000;
    const SHIFTED_EXPONENT_MASK: u64 = 0x7F80_0000;
    fn from_uint(bits: u64) -> Self {
        f32::from_bits(bits as u32)
    }
    fn zero() -> Self {
        0.0
    }
}
impl FromCharsFloat for f32 {}

impl private::Floating for f64 {
    const MANTISSA_BITS: i32 = 53;
    const MAXIMUM_BINARY_EXPONENT: i32 = 1023;
    const MINIMUM_BINARY_EXPONENT: i32 = -1022;
    const EXPONENT_BIAS: i32 = 1023;
    const SIGN_SHIFT: u32 = 63;
    const EXPONENT_SHIFT: u32 = 52;
    const NORMAL_MANTISSA_MASK: u64 = 0x001F_FFFF_FFFF_FFFF;
    const DENORMAL_MANTISSA_MASK: u64 = 0x000F_FFFF_FFFF_FFFF;
    const SPECIAL_NAN_MANTISSA_MASK: u64 = 0x0008_0000_0000_0000;
    const SHIFTED_SIGN_MASK: u64 = 0x8000_0000_0000_0000;
    const SHIFTED_EXPONENT_MASK: u64 = 0x7FF0_0000_0000_0000;
    fn from_uint(bits: u64) -> Self {
        f64::from_bits(bits)
    }
    fn zero() -> Self {
        0.0
    }
}
impl FromCharsFloat for f64 {}

const fn invalid_argument(ptr: usize) -> FromCharsResult {
    FromCharsResult {
        ptr,
        ec: Some(Errc::InvalidArgument),
    }
}

/// `_Digit_from_char`: `0`-`9`, `A`-`Z` and `a`-`z` to 0 to 35, everything else to 255.
const fn digit_from_char(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'A'..=b'Z' => c - b'A' + 10,
        b'a'..=b'z' => c - b'a' + 10,
        _ => 255,
    }
}

/// The low `bits` bits set.
const fn uint_max(bits: u32) -> u64 {
    if bits == 64 {
        u64::MAX
    } else {
        (1 << bits) - 1
    }
}

/// `std::from_chars(first, last, value, base)` for an integer type: an optional `-` (signed types
/// only) and digits of `base`.
///
/// # Panics
///
/// If `base` is not in 2 to 36, the precondition of the C++ function.
pub fn from_chars_int<T: FromCharsInteger>(s: &[u8], value: &mut T, base: u32) -> FromCharsResult {
    assert!((2..=36).contains(&base), "invalid base in from_chars()");

    let mut minus_sign = false;
    let mut next = 0;
    if T::SIGNED && s.first() == Some(&b'-') {
        minus_sign = true;
        next += 1;
    }

    let uint_max = uint_max(T::BITS);
    let int_max = uint_max >> 1;
    let abs_int_min = int_max + 1;
    let base = u64::from(base);

    let (risky_val, max_digit) = if T::SIGNED {
        if minus_sign {
            (abs_int_min / base, abs_int_min % base)
        } else {
            (int_max / base, int_max % base)
        }
    } else {
        (uint_max / base, uint_max % base)
    };

    let mut val: u64 = 0;
    let mut overflowed = false;
    while let Some(&c) = s.get(next) {
        let digit = u64::from(digit_from_char(c));
        if digit >= base {
            break;
        }
        if val < risky_val || (val == risky_val && digit <= max_digit) {
            val = val * base + digit;
        } else {
            // Keep going: next still needs to be updated; val is now irrelevant.
            overflowed = true;
        }
        next += 1;
    }

    if next == usize::from(minus_sign) {
        return invalid_argument(0);
    }
    if overflowed {
        return FromCharsResult {
            ptr: next,
            ec: Some(Errc::ResultOutOfRange),
        };
    }
    if T::SIGNED && minus_sign {
        val = 0u64.wrapping_sub(val) & uint_max;
    }
    *value = T::from_raw(val);
    FromCharsResult {
        ptr: next,
        ec: None,
    }
}

/// `_Big_integer_flt::_Maximum_bits`: 2^1074's bits, log2(10^768)'s and 54 for the shift.
const MAXIMUM_BITS: u32 = 1074 + 2552 + 54;
const ELEMENT_BITS: u32 = 32;
const ELEMENT_COUNT: u32 = MAXIMUM_BITS.div_ceil(ELEMENT_BITS);

/// `_Big_integer_flt`: an unsigned integer of up to `ELEMENT_COUNT` 32-bit limbs, little-endian,
/// `used` of them in use. (The C++ leaves the unused limbs uninitialised and never reads them;
/// here they are zero.)
#[derive(Clone)]
struct BigInteger {
    used: u32,
    data: [u32; ELEMENT_COUNT as usize],
}

impl BigInteger {
    fn new() -> BigInteger {
        BigInteger {
            used: 0,
            data: [0; ELEMENT_COUNT as usize],
        }
    }

    /// `_Make_big_integer_flt_one`.
    fn one() -> BigInteger {
        let mut x = BigInteger::new();
        x.data[0] = 1;
        x.used = 1;
        x
    }

    /// `operator<`.
    fn less_than(&self, rhs: &BigInteger) -> bool {
        if self.used != rhs.used {
            return self.used < rhs.used;
        }
        for ix in (0..self.used as usize).rev() {
            if self.data[ix] != rhs.data[ix] {
                return self.data[ix] < rhs.data[ix];
            }
        }
        false
    }
}

/// `_Bit_scan_reverse(uint32_t)`: the index of the highest set bit plus 1, or 0.
const fn bit_scan_reverse_32(value: u32) -> u32 {
    32 - value.leading_zeros()
}

/// `_Bit_scan_reverse(uint64_t)`.
const fn bit_scan_reverse_64(value: u64) -> u32 {
    64 - value.leading_zeros()
}

/// `_Bit_scan_reverse(const _Big_integer_flt&)`.
fn bit_scan_reverse_big(x: &BigInteger) -> u32 {
    if x.used == 0 {
        return 0;
    }
    let bx = x.used - 1;
    bit_scan_reverse_32(x.data[bx as usize]) + bx * ELEMENT_BITS
}

/// `_Shift_left`: false (and zero) on overflow.
fn shift_left(x: &mut BigInteger, nx: u32) -> bool {
    if x.used == 0 {
        return true;
    }

    let unit_shift = nx / ELEMENT_BITS;
    let bit_shift = nx % ELEMENT_BITS;

    if x.used + unit_shift > ELEMENT_COUNT {
        x.used = 0;
        return false;
    }

    if bit_shift == 0 {
        x.data.copy_within(0..x.used as usize, unit_shift as usize);
        x.used += unit_shift;
    } else {
        let bit_shifts_into_next_unit =
            bit_shift > (ELEMENT_BITS - bit_scan_reverse_32(x.data[x.used as usize - 1]));

        let new_used = x.used + unit_shift + u32::from(bit_shifts_into_next_unit);

        if new_used > ELEMENT_COUNT {
            x.used = 0;
            return false;
        }

        let msb_bits = bit_shift;
        let lsb_bits = ELEMENT_BITS - msb_bits;

        let lsb_mask = (1u32 << lsb_bits) - 1;
        let msb_mask = !lsb_mask;

        // If unit_shift == 0, this wraps around, which is okay.
        let mut dest_index = new_used - 1;
        while dest_index != unit_shift.wrapping_sub(1) {
            let upper_source_index = dest_index - unit_shift;
            // When dest_index == unit_shift, this wraps around (see the bounds check below).
            let lower_source_index = dest_index.wrapping_sub(unit_shift).wrapping_sub(1);

            let upper_source = if upper_source_index < x.used {
                x.data[upper_source_index as usize]
            } else {
                0
            };
            let lower_source = if lower_source_index < x.used {
                x.data[lower_source_index as usize]
            } else {
                0
            };

            let shifted_upper_source = (upper_source & lsb_mask) << msb_bits;
            let shifted_lower_source = (lower_source & msb_mask) >> lsb_bits;

            x.data[dest_index as usize] = shifted_upper_source | shifted_lower_source;
            dest_index = dest_index.wrapping_sub(1);
        }

        x.used = new_used;
    }

    x.data[..unit_shift as usize].fill(0);
    true
}

/// `_Add` of a 32-bit value: false (and zero) on overflow.
fn add(x: &mut BigInteger, value: u32) -> bool {
    if value == 0 {
        return true;
    }

    let mut carry = value;
    for limb in &mut x.data[..x.used as usize] {
        let result = u64::from(*limb) + u64::from(carry);
        *limb = result as u32;
        carry = (result >> 32) as u32;
    }

    if carry != 0 {
        if x.used < ELEMENT_COUNT {
            x.data[x.used as usize] = carry;
            x.used += 1;
        } else {
            x.used = 0;
            return false;
        }
    }
    true
}

/// `_Add_carry`.
fn add_carry(u1: &mut u32, u2: u32, carry: u32) -> u32 {
    let uu = u64::from(*u1) + u64::from(u2) + u64::from(carry);
    *u1 = uu as u32;
    (uu >> 32) as u32
}

/// `_Add_multiply_carry`.
fn add_multiply_carry(add: &mut u32, mul_1: u32, mul_2: u32, carry: u32) -> u32 {
    let uu_res = u64::from(mul_1) * u64::from(mul_2) + u64::from(*add) + u64::from(carry);
    *add = uu_res as u32;
    (uu_res >> 32) as u32
}

/// `_Multiply_core`.
fn multiply_core(multiplicand: &mut [u32], multiplier: u32) -> u32 {
    let mut carry = 0;
    for limb in multiplicand {
        let result = u64::from(*limb) * u64::from(multiplier) + u64::from(carry);
        *limb = result as u32;
        carry = (result >> 32) as u32;
    }
    carry
}

/// `_Multiply` by a 32-bit value: false (and zero) on overflow.
fn multiply(multiplicand: &mut BigInteger, multiplier: u32) -> bool {
    if multiplier == 0 {
        multiplicand.used = 0;
        return true;
    }
    if multiplier == 1 {
        return true;
    }
    if multiplicand.used == 0 {
        return true;
    }

    let used = multiplicand.used as usize;
    let carry = multiply_core(&mut multiplicand.data[..used], multiplier);
    if carry != 0 {
        if multiplicand.used < ELEMENT_COUNT {
            multiplicand.data[used] = carry;
            multiplicand.used += 1;
        } else {
            multiplicand.used = 0;
            return false;
        }
    }
    true
}

/// `_Multiply` by a big integer (the STL's translation of .NET's `BigIntegerBuilder.Mul`): false
/// (and zero) on overflow.
fn multiply_big(multiplicand: &mut BigInteger, multiplier: &BigInteger) -> bool {
    if multiplicand.used == 0 {
        return true;
    }
    if multiplier.used == 0 {
        multiplicand.used = 0;
        return true;
    }
    if multiplier.used == 1 {
        return multiply(multiplicand, multiplier.data[0]);
    }
    if multiplicand.used == 1 {
        let small_multiplier = multiplicand.data[0];
        *multiplicand = multiplier.clone();
        return multiply(multiplicand, small_multiplier);
    }

    // More iterations on the inner loop and fewer on the outer.
    let multiplier_is_shorter = multiplier.used < multiplicand.used;
    let (rgu1, rgu2) = if multiplier_is_shorter {
        (&multiplier.data, &multiplicand.data)
    } else {
        (&multiplicand.data, &multiplier.data)
    };
    let (cu1, cu2) = if multiplier_is_shorter {
        (multiplier.used, multiplicand.used)
    } else {
        (multiplicand.used, multiplier.used)
    };

    let mut result = BigInteger::new();
    for iu1 in 0..cu1 {
        let u_cur = rgu1[iu1 as usize];
        if u_cur == 0 {
            if iu1 == result.used {
                result.data[iu1 as usize] = 0;
                result.used = iu1 + 1;
            }
            continue;
        }

        let mut u_carry = 0;
        let mut iu_res = iu1;
        let mut iu2 = 0;
        while iu2 != cu2 && iu_res != ELEMENT_COUNT {
            if iu_res == result.used {
                result.data[iu_res as usize] = 0;
                result.used = iu_res + 1;
            }
            u_carry = add_multiply_carry(
                &mut result.data[iu_res as usize],
                u_cur,
                rgu2[iu2 as usize],
                u_carry,
            );
            iu2 += 1;
            iu_res += 1;
        }

        while u_carry != 0 && iu_res != ELEMENT_COUNT {
            if iu_res == result.used {
                result.data[iu_res as usize] = 0;
                result.used = iu_res + 1;
            }
            u_carry = add_carry(&mut result.data[iu_res as usize], 0, u_carry);
            iu_res += 1;
        }

        if iu_res == ELEMENT_COUNT {
            multiplicand.used = 0;
            return false;
        }
    }

    *multiplicand = result;
    true
}

/// `_Large_power_indices`: where 10^10, 10^20, ... 10^380 start in `LARGE_POWER_DATA`, how many
/// low zero limbs each lost and how many limbs it has.
const LARGE_POWER_INDICES: [(u16, u8, u8); 38] = [
    (0, 0, 2),
    (2, 0, 3),
    (5, 0, 4),
    (9, 1, 4),
    (13, 1, 5),
    (18, 1, 6),
    (24, 2, 6),
    (30, 2, 7),
    (37, 2, 8),
    (45, 3, 8),
    (53, 3, 9),
    (62, 3, 10),
    (72, 4, 10),
    (82, 4, 11),
    (93, 4, 12),
    (105, 5, 12),
    (117, 5, 13),
    (130, 5, 14),
    (144, 5, 15),
    (159, 6, 15),
    (174, 6, 16),
    (190, 6, 17),
    (207, 7, 17),
    (224, 7, 18),
    (242, 7, 19),
    (261, 8, 19),
    (280, 8, 21),
    (301, 8, 22),
    (323, 9, 22),
    (345, 9, 23),
    (368, 9, 24),
    (392, 10, 24),
    (416, 10, 25),
    (441, 10, 26),
    (467, 10, 27),
    (494, 11, 27),
    (521, 11, 28),
    (549, 11, 29),
];

/// `_Multiply_by_power_of_ten`: false (and zero) on overflow.
fn multiply_by_power_of_ten(x: &mut BigInteger, power: u32) -> bool {
    let mut large_power = power / 10;
    while large_power != 0 {
        let current_power = large_power.min(LARGE_POWER_INDICES.len() as u32);
        let (offset, zeroes, size) = LARGE_POWER_INDICES[current_power as usize - 1];
        let (offset, zeroes, size) = (usize::from(offset), usize::from(zeroes), usize::from(size));

        let mut multiplier = BigInteger::new();
        multiplier.used = (size + zeroes) as u32;
        multiplier.data[zeroes..zeroes + size]
            .copy_from_slice(&LARGE_POWER_DATA[offset..offset + size]);

        if !multiply_big(x, &multiplier) {
            return false;
        }
        large_power -= current_power;
    }

    const SMALL_POWERS_OF_TEN: [u32; 9] = [
        10,
        100,
        1_000,
        10_000,
        100_000,
        1_000_000,
        10_000_000,
        100_000_000,
        1_000_000_000,
    ];

    let small_power = power % 10;
    if small_power == 0 {
        return true;
    }
    multiply(x, SMALL_POWERS_OF_TEN[small_power as usize - 1])
}

/// `_Count_sequential_high_zeroes`.
const fn count_sequential_high_zeroes(u: u32) -> u32 {
    u.leading_zeros()
}

/// Whose floating-point parser a parse follows. The UCRT's `strtod` and `strtof` work as the
/// STL's `from_chars` does, in the default rounding mode the engine keeps, except in three ways,
/// each compared with the UCRT itself in `tests/fmt.rs`:
///
/// - the digits after the 768th are dropped without a note of whether any of them was nonzero, so
///   a longer input rounds as its first 768 digits do;
/// - a zero remainder of the fractional part's division still counts as nonzero when the
///   denominator fits one 32-bit element and the numerator does not, so an exact tie there rounds
///   up, not to even: `strtof("52159.384765625")` is 52159.3867, `from_chars` gives 52159.3828;
/// - the exponent is read up to 5201 and no further, and checked against ±5200 before the place of
///   the point is added to it, so `0.` followed by 6000 zeros and `1e6000` is an overflow.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Parser {
    /// `std::from_chars`.
    Stl,
    /// The UCRT's `strtod` and `strtof`.
    Ucrt,
}

/// `_Divide` (the STL's translation of .NET's `BigIntegerBuilder.ModDivCore`): leaves the
/// remainder in `numerator` and returns the low 64 bits of the quotient.
fn divide(numerator: &mut BigInteger, denominator: &BigInteger, parser: Parser) -> u64 {
    if numerator.used == 0 {
        return 0;
    }

    let mut max_numerator_element_index = numerator.used - 1;
    let max_denominator_element_index = denominator.used - 1;

    // A one-element denominator takes the fast route.
    if max_denominator_element_index == 0 {
        let small_denominator = denominator.data[0];

        if max_numerator_element_index == 0 {
            let small_numerator = numerator.data[0];

            if small_denominator == 1 {
                numerator.used = 0;
                return u64::from(small_numerator);
            }

            numerator.data[0] = small_numerator % small_denominator;
            numerator.used = u32::from(numerator.data[0] > 0);
            return u64::from(small_numerator / small_denominator);
        }

        if small_denominator == 1 {
            let mut quotient = u64::from(numerator.data[1]);
            quotient <<= 32;
            quotient |= u64::from(numerator.data[0]);
            numerator.used = 0;
            return quotient;
        }

        // Counting down, the last assignment to quotient is the right one.
        let mut quotient: u64 = 0;
        let mut uu: u64 = 0;
        let small_denominator = u64::from(small_denominator);
        let mut iv = max_numerator_element_index;
        while iv != u32::MAX {
            uu = (uu << 32) | u64::from(numerator.data[iv as usize]);
            quotient = (quotient << 32).wrapping_add(u64::from((uu / small_denominator) as u32));
            uu %= small_denominator;
            iv = iv.wrapping_sub(1);
        }

        numerator.data[1] = (uu >> 32) as u32;
        numerator.data[0] = uu as u32;
        numerator.used = if numerator.data[1] > 0 {
            2
        } else if numerator.data[0] > 0 || parser == Parser::Ucrt {
            // The UCRT keeps one element even for a zero remainder (see Parser).
            1
        } else {
            0
        };
        return quotient;
    }

    if max_denominator_element_index > max_numerator_element_index {
        return 0;
    }

    let cu_den = max_denominator_element_index + 1;
    let cu_diff = (max_numerator_element_index - max_denominator_element_index) as i32;

    // Whether the result has cu_diff or cu_diff + 1 digits.
    let mut cu_quo = cu_diff;
    let mut iu = max_numerator_element_index as i32;
    loop {
        if iu < cu_diff {
            cu_quo += 1;
            break;
        }
        let d = denominator.data[(iu - cu_diff) as usize];
        let n = numerator.data[iu as usize];
        if d != n {
            if d < n {
                cu_quo += 1;
            }
            break;
        }
        iu -= 1;
    }

    if cu_quo == 0 {
        return 0;
    }

    // The trial divisor, normalised so its high bit is set.
    let mut u_den = denominator.data[cu_den as usize - 1];
    let mut u_den_next = denominator.data[cu_den as usize - 2];

    let cbit_shift_left = count_sequential_high_zeroes(u_den);
    let cbit_shift_right = 32 - cbit_shift_left;
    if cbit_shift_left > 0 {
        u_den = (u_den << cbit_shift_left) | (u_den_next >> cbit_shift_right);
        u_den_next <<= cbit_shift_left;

        if cu_den > 2 {
            u_den_next |= denominator.data[cu_den as usize - 3] >> cbit_shift_right;
        }
    }

    let mut quotient: u64 = 0;
    let mut iu = cu_quo;
    loop {
        iu -= 1;
        if iu < 0 {
            break;
        }
        let iu_u = iu as u32;

        // The high (normalised) bits of the numerator.
        let u_num_hi = if iu_u + cu_den <= max_numerator_element_index {
            numerator.data[(iu_u + cu_den) as usize]
        } else {
            0
        };

        let mut uu_num =
            (u64::from(u_num_hi) << 32) | u64::from(numerator.data[(iu_u + cu_den - 1) as usize]);

        let mut u_num_next = numerator.data[(iu_u + cu_den - 2) as usize];
        if cbit_shift_left > 0 {
            uu_num = (uu_num << cbit_shift_left) | u64::from(u_num_next >> cbit_shift_right);
            u_num_next <<= cbit_shift_left;

            if iu_u + cu_den >= 3 {
                u_num_next |= numerator.data[(iu_u + cu_den - 3) as usize] >> cbit_shift_right;
            }
        }

        // The quotient digit.
        let mut uu_quo = uu_num / u64::from(u_den);
        let mut uu_rem = u64::from((uu_num % u64::from(u_den)) as u32);

        if uu_quo > u64::from(u32::MAX) {
            uu_rem =
                uu_rem.wrapping_add(u64::from(u_den).wrapping_mul(uu_quo - u64::from(u32::MAX)));
            uu_quo = u64::from(u32::MAX);
        }

        while uu_rem <= u64::from(u32::MAX)
            && uu_quo.wrapping_mul(u64::from(u_den_next)) > ((uu_rem << 32) | u64::from(u_num_next))
        {
            uu_quo -= 1;
            uu_rem = uu_rem.wrapping_add(u64::from(u_den));
        }

        // Multiply and subtract; uu_quo may be one too large, in which case the denominator is
        // added back.
        if uu_quo > 0 {
            let mut uu_borrow: u64 = 0;

            for iu2 in 0..cu_den {
                uu_borrow = uu_borrow
                    .wrapping_add(uu_quo.wrapping_mul(u64::from(denominator.data[iu2 as usize])));

                let u_sub = uu_borrow as u32;
                uu_borrow >>= 32;
                let limb = &mut numerator.data[(iu_u + iu2) as usize];
                if *limb < u_sub {
                    uu_borrow += 1;
                }
                *limb = limb.wrapping_sub(u_sub);
            }

            if u64::from(u_num_hi) < uu_borrow {
                let mut u_carry: u32 = 0;
                for iu2 in 0..cu_den {
                    let limb = &mut numerator.data[(iu_u + iu2) as usize];
                    let sum = u64::from(*limb)
                        + u64::from(denominator.data[iu2 as usize])
                        + u64::from(u_carry);
                    *limb = sum as u32;
                    u_carry = (sum >> 32) as u32;
                }
                uu_quo -= 1;
            }

            max_numerator_element_index = iu_u + cu_den - 1;
        }

        quotient = (quotient << 32).wrapping_add(u64::from(uu_quo as u32));
    }

    // Trim the remainder.
    let mut used = max_numerator_element_index + 1;
    while used != 0 && numerator.data[used as usize - 1] == 0 {
        used -= 1;
    }
    numerator.used = used;

    quotient
}

/// `_Floating_point_string`: `[sign] 0.mantissa * B^exponent`, the mantissa's digits one per
/// byte, at most 768 of them.
struct FloatingPointString {
    is_negative: bool,
    exponent: i32,
    mantissa_count: u32,
    mantissa: [u8; 768],
}

/// `_Assemble_floating_point_zero`.
fn assemble_zero<T: FromCharsFloat>(is_negative: bool) -> T {
    T::from_uint(u64::from(is_negative) << T::SIGN_SHIFT)
}

/// `_Assemble_floating_point_infinity`.
fn assemble_infinity<T: FromCharsFloat>(is_negative: bool) -> T {
    T::from_uint((u64::from(is_negative) << T::SIGN_SHIFT) | T::SHIFTED_EXPONENT_MASK)
}

/// `_Right_shift_with_rounding`: `value / 2^shift`, rounded to nearest, ties to even;
/// `has_zero_tail` says whether every bit already dropped was zero.
fn right_shift_with_rounding(value: u64, shift: u32, has_zero_tail: bool) -> u64 {
    const TOTAL_NUMBER_OF_BITS: u32 = 64;
    if shift >= TOTAL_NUMBER_OF_BITS {
        if shift == TOTAL_NUMBER_OF_BITS {
            const EXTRA_BITS_MASK: u64 = (1 << (TOTAL_NUMBER_OF_BITS - 1)) - 1;
            const ROUND_BIT_MASK: u64 = 1 << (TOTAL_NUMBER_OF_BITS - 1);

            let round_bit = (value & ROUND_BIT_MASK) != 0;
            let tail_bits = !has_zero_tail || (value & EXTRA_BITS_MASK) != 0;

            // Rounds up to 1 above 0.5, down to 0 below it and at it.
            return u64::from(round_bit && tail_bits);
        }
        // A shift of 65 or more: the answer is less than 0.5.
        return 0;
    }

    let lsb_bit = value;
    let round_bit = value << 1;
    let has_tail_bits = round_bit.wrapping_sub(u64::from(has_zero_tail));
    let should_round = ((round_bit & (has_tail_bits | lsb_bit)) >> shift) & 1;
    (value >> shift) + should_round
}

/// `_Assemble_floating_point_value_no_shift`: `[sign] (mantissa / 2^(precision - 1)) *
/// 2^exponent`, the mantissa already rounded, a carry into a higher bit (and so into infinity)
/// included.
fn assemble_no_shift<T: FromCharsFloat>(is_negative: bool, exponent: i32, mantissa: u64) -> T {
    let sign_component = u64::from(is_negative) << T::SIGN_SHIFT;
    let exponent_component =
        u64::from((exponent + (T::EXPONENT_BIAS - 1)) as u32) << T::EXPONENT_SHIFT;
    T::from_uint(sign_component | exponent_component.wrapping_add(mantissa))
}

/// `_Assemble_floating_point_value`: rounds the mantissa to the type's precision (and to a
/// subnormal one), reporting an overflow to infinity and a result that rounds to zero.
fn assemble_value<T: FromCharsFloat>(
    initial_mantissa: u64,
    initial_exponent: i32,
    is_negative: bool,
    has_zero_tail: bool,
) -> (T, Option<Errc>) {
    // Assume the number is a normal value: the shift that puts the mantissa in place, and the
    // exponent of the normalised mantissa.
    let initial_mantissa_bits = bit_scan_reverse_64(initial_mantissa);
    let normal_mantissa_shift = T::MANTISSA_BITS - initial_mantissa_bits as i32;
    let normal_exponent = initial_exponent - normal_mantissa_shift;

    if normal_exponent > T::MAXIMUM_BINARY_EXPONENT {
        // Overflow example: "1e+1000".
        return (assemble_infinity(is_negative), Some(Errc::ResultOutOfRange));
    }

    let mut mantissa = initial_mantissa;
    let mut exponent = normal_exponent;
    let mut error_code = None;

    if normal_exponent < T::MINIMUM_BINARY_EXPONENT {
        // Too small for a normal value; perhaps a subnormal one, whose exponent is the minimum.
        exponent = T::MINIMUM_BINARY_EXPONENT;

        let denormal_mantissa_shift = initial_exponent - exponent;

        if denormal_mantissa_shift < 0 {
            mantissa = right_shift_with_rounding(
                mantissa,
                (-denormal_mantissa_shift) as u32,
                has_zero_tail,
            );

            // MSVC's from_chars and the UCRT's strtod report underflow only for a result that
            // rounds to zero (underflow example: "1e-1000"). A subnormal that rounds up into a
            // normal value is handled by assemble_no_shift.
            if mantissa == 0 {
                error_code = Some(Errc::ResultOutOfRange);
            }
        } else {
            mantissa <<= denormal_mantissa_shift;
        }
    } else if normal_mantissa_shift < 0 {
        mantissa =
            right_shift_with_rounding(mantissa, (-normal_mantissa_shift) as u32, has_zero_tail);

        // Rounding may carry into the next exponent (assemble_no_shift), and past the largest.
        // Overflow example: "1.fffffffffffff8p+1023" for double.
        if mantissa > T::NORMAL_MANTISSA_MASK && exponent == T::MAXIMUM_BINARY_EXPONENT {
            error_code = Some(Errc::ResultOutOfRange);
        }
    } else {
        mantissa <<= normal_mantissa_shift;
    }

    (
        assemble_no_shift(is_negative, exponent, mantissa),
        error_code,
    )
}

/// `_Assemble_floating_point_value_from_big_integer_flt`: an integer value (the fast track for an
/// integer, or an integer part with all the precision needed).
fn assemble_value_from_big_integer<T: FromCharsFloat>(
    integer_value: &BigInteger,
    integer_bits_of_precision: u32,
    is_negative: bool,
    has_nonzero_fractional_part: bool,
) -> (T, Option<Errc>) {
    let base_exponent = T::MANTISSA_BITS - 1;

    // 64 bits or fewer: the two low limbs.
    if integer_bits_of_precision <= 64 {
        let exponent = base_exponent;

        let mantissa_low = if integer_value.used > 0 {
            integer_value.data[0]
        } else {
            0
        };
        let mantissa_high = if integer_value.used > 1 {
            integer_value.data[1]
        } else {
            0
        };
        let mantissa = u64::from(mantissa_low) + (u64::from(mantissa_high) << 32);

        return assemble_value(
            mantissa,
            exponent,
            is_negative,
            !has_nonzero_fractional_part,
        );
    }

    let top_element_bits = integer_bits_of_precision % 32;
    let top_element_index = integer_bits_of_precision / 32;

    let middle_element_index = top_element_index - 1;
    let bottom_element_index = top_element_index - 2;

    // The top 64 bits in two limbs.
    if top_element_bits == 0 {
        let exponent = (base_exponent as u32 + bottom_element_index * 32) as i32;

        let mantissa = u64::from(integer_value.data[bottom_element_index as usize])
            + (u64::from(integer_value.data[middle_element_index as usize]) << 32);

        let mut has_zero_tail = !has_nonzero_fractional_part;
        let mut ix = 0;
        while has_zero_tail && ix != bottom_element_index {
            has_zero_tail = integer_value.data[ix as usize] == 0;
            ix += 1;
        }

        return assemble_value(mantissa, exponent, is_negative, has_zero_tail);
    }

    // The top 64 bits span three limbs.
    let top_element_mask = (1u32 << top_element_bits) - 1;
    let top_element_shift = 64 - top_element_bits;

    let middle_element_shift = top_element_shift - 32;

    let bottom_element_bits = 32 - top_element_bits;
    let bottom_element_mask = !top_element_mask;
    let bottom_element_shift = 32 - bottom_element_bits;

    let exponent = (base_exponent as u32 + bottom_element_index * 32 + top_element_bits) as i32;

    let mantissa = (u64::from(integer_value.data[top_element_index as usize] & top_element_mask)
        << top_element_shift)
        + (u64::from(integer_value.data[middle_element_index as usize]) << middle_element_shift)
        + (u64::from(integer_value.data[bottom_element_index as usize] & bottom_element_mask)
            >> bottom_element_shift);

    let mut has_zero_tail = !has_nonzero_fractional_part
        && (integer_value.data[bottom_element_index as usize] & top_element_mask) == 0;

    let mut ix = 0;
    while has_zero_tail && ix != bottom_element_index {
        has_zero_tail = integer_value.data[ix as usize] == 0;
        ix += 1;
    }

    assemble_value(mantissa, exponent, is_negative, has_zero_tail)
}

/// `_Accumulate_decimal_digits_into_big_integer_flt`: nine digits at a time.
fn accumulate_decimal_digits(digits: &[u8], result: &mut BigInteger) {
    let mut accumulator: u32 = 0;
    let mut accumulator_count: u32 = 0;
    for &digit in digits {
        if accumulator_count == 9 {
            // The buffer's 768 digits never overflow the big integer.
            let multiplied = multiply(result, 1_000_000_000);
            debug_assert!(multiplied);
            let added = add(result, accumulator);
            debug_assert!(added);

            accumulator = 0;
            accumulator_count = 0;
        }

        accumulator *= 10;
        accumulator += u32::from(digit);
        accumulator_count += 1;
    }

    if accumulator_count != 0 {
        let multiplied = multiply_by_power_of_ten(result, accumulator_count);
        debug_assert!(multiplied);
        let added = add(result, accumulator);
        debug_assert!(added);
    }
}

/// `_Convert_decimal_string_to_floating_type`.
fn convert_decimal_string<T: FromCharsFloat>(
    data: &FloatingPointString,
    mut has_zero_tail: bool,
    parser: Parser,
) -> (T, Option<Errc>) {
    // An N-bit mantissa takes N + 1 bits of precision: the extra one rounds.
    let required_bits_of_precision = (T::MANTISSA_BITS + 1) as u32;

    // 0.mantissa * 10^exponent: the first `exponent` digits (if positive) are the integer part,
    // the rest the fractional part.
    let positive_exponent = data.exponent.max(0) as u32;
    let integer_digits_present = positive_exponent.min(data.mantissa_count);
    let integer_digits_missing = positive_exponent - integer_digits_present;
    let integer_digits = &data.mantissa[..integer_digits_present as usize];
    let fractional_digits =
        &data.mantissa[integer_digits_present as usize..data.mantissa_count as usize];
    let fractional_digits_present = fractional_digits.len() as u32;

    // The integer part.
    let mut integer_value = BigInteger::new();
    accumulate_decimal_digits(integer_digits, &mut integer_value);

    if integer_digits_missing > 0
        && !multiply_by_power_of_ten(&mut integer_value, integer_digits_missing)
    {
        // Overflow example: "1e+2000".
        return (
            assemble_infinity(data.is_negative),
            Some(Errc::ResultOutOfRange),
        );
    }

    // With enough bits from the integer part, or no fractional part, the result is ready.
    let integer_bits_of_precision = bit_scan_reverse_big(&integer_value);
    {
        let has_zero_fractional_part = fractional_digits_present == 0 && has_zero_tail;

        if integer_bits_of_precision >= required_bits_of_precision || has_zero_fractional_part {
            return assemble_value_from_big_integer(
                &integer_value,
                integer_bits_of_precision,
                data.is_negative,
                !has_zero_fractional_part,
            );
        }
    }

    // Otherwise the fractional part gives more bits: as a fraction N/M, M a power of 10.
    let mut fractional_numerator = BigInteger::new();
    accumulate_decimal_digits(fractional_digits, &mut fractional_numerator);

    let fractional_denominator_exponent = if data.exponent < 0 {
        fractional_digits_present + (-data.exponent) as u32
    } else {
        fractional_digits_present
    };

    let mut fractional_denominator = BigInteger::one();
    if !multiply_by_power_of_ten(&mut fractional_denominator, fractional_denominator_exponent) {
        // With integer digits this cannot underflow, so it is a true underflow to zero.
        // Underflow example: "1e-2000".
        return (
            assemble_zero(data.is_negative),
            Some(Errc::ResultOutOfRange),
        );
    }

    // N < M. Normalise so that N's highest bit is where M's is, so that a shift of N by k bits
    // gives k bits of precision.
    let fractional_numerator_bits = bit_scan_reverse_big(&fractional_numerator);
    let fractional_denominator_bits = bit_scan_reverse_big(&fractional_denominator);

    let fractional_shift = fractional_denominator_bits.saturating_sub(fractional_numerator_bits);

    if fractional_shift > 0 {
        let shifted = shift_left(&mut fractional_numerator, fractional_shift);
        debug_assert!(shifted);
    }

    let required_fractional_bits_of_precision =
        required_bits_of_precision - integer_bits_of_precision;

    let mut remaining_bits_of_precision_required = required_fractional_bits_of_precision;
    if integer_bits_of_precision > 0 {
        // If the fractional part gives no bits and cannot affect rounding (5.0000000000000000000001),
        // the integer part's bits are the result. When the shift equals the bits required, the
        // fraction still decides the rounding, so the division is done.
        if fractional_shift > remaining_bits_of_precision_required {
            return assemble_value_from_big_integer(
                &integer_value,
                integer_bits_of_precision,
                data.is_negative,
                fractional_digits_present != 0 || !has_zero_tail,
            );
        }

        remaining_bits_of_precision_required -= fractional_shift;
    }

    // Without an integer part the exponent comes from the fraction: the shift, or one more.
    let fractional_exponent = if fractional_numerator.less_than(&fractional_denominator) {
        fractional_shift + 1
    } else {
        fractional_shift
    };

    // The STL only checks this shift in its own debug builds. It fails, and leaves N zero, only
    // for a denominator within 54 bits of the big integer's 3680, a value far below the least
    // subnormal: the zero mantissa then underflows to zero, as the value does.
    let _ = shift_left(
        &mut fractional_numerator,
        remaining_bits_of_precision_required,
    );

    let mut fractional_mantissa =
        divide(&mut fractional_numerator, &fractional_denominator, parser);

    has_zero_tail = has_zero_tail && fractional_numerator.used == 0;

    // Drop any bits beyond those required.
    let fractional_mantissa_bits = bit_scan_reverse_64(fractional_mantissa);
    if fractional_mantissa_bits > required_fractional_bits_of_precision {
        let shift = fractional_mantissa_bits - required_fractional_bits_of_precision;
        has_zero_tail = has_zero_tail && (fractional_mantissa & ((1u64 << shift) - 1)) == 0;
        fractional_mantissa >>= shift;
    }

    // The mantissa from the integer and fractional parts.
    let integer_mantissa_low = if integer_value.used > 0 {
        integer_value.data[0]
    } else {
        0
    };
    let integer_mantissa_high = if integer_value.used > 1 {
        integer_value.data[1]
    } else {
        0
    };
    let integer_mantissa =
        u64::from(integer_mantissa_low) + (u64::from(integer_mantissa_high) << 32);

    let complete_mantissa =
        (integer_mantissa << required_fractional_bits_of_precision) + fractional_mantissa;

    // With an integer part, the exponent is its bits less one (for the form 1.xxx); without, the
    // fractional exponent; and one less for the extra rounding bit.
    let final_exponent = if integer_bits_of_precision > 0 {
        // static_cast<int32_t> of the uint32_t difference: -1 for a single integer bit.
        integer_bits_of_precision.wrapping_sub(2) as i32
    } else {
        -(fractional_exponent as i32) - 1
    };

    assemble_value(
        complete_mantissa,
        final_exponent,
        data.is_negative,
        has_zero_tail,
    )
}

/// `_Convert_hexadecimal_string_to_floating_type`.
fn convert_hexadecimal_string<T: FromCharsFloat>(
    data: &FloatingPointString,
    mut has_zero_tail: bool,
) -> (T, Option<Errc>) {
    let mut mantissa: u64 = 0;
    let mut exponent = data.exponent + T::MANTISSA_BITS - 1;

    let digits = &data.mantissa[..data.mantissa_count as usize];
    let mut it = 0;
    while it != digits.len() && mantissa <= T::NORMAL_MANTISSA_MASK {
        mantissa *= 16;
        mantissa += u64::from(digits[it]);
        it += 1;
        // The exponent is binary: log2(16) == 4.
        exponent -= 4;
    }

    while has_zero_tail && it != digits.len() {
        has_zero_tail = digits[it] == 0;
        it += 1;
    }

    assemble_value(mantissa, exponent, data.is_negative, has_zero_tail)
}

/// `_Ordinary_floating_from_chars`: digits, an optional point, more digits and an exponent, from
/// `s[next..]`, with the sign already read.
fn ordinary_floating_from_chars<T: FromCharsFloat>(
    s: &[u8],
    value: &mut T,
    format: CharsFormat,
    minus_sign: bool,
    mut next: usize,
    parser: Parser,
) -> FromCharsResult {
    let is_hexadecimal = format == CharsFormat::Hex;
    let base: u8 = if is_hexadecimal { 16 } else { 10 };

    let mut fp_string = FloatingPointString {
        is_negative: minus_sign,
        exponent: 0,
        mantissa_count: 0,
        mantissa: [0; 768],
    };
    let mantissa_last = fp_string.mantissa.len();
    let mut mantissa_it = 0;

    // [whole_begin, whole_end): 0 or more digits.
    let whole_begin = next;

    // Leading zeroes.
    while s.get(next) == Some(&b'0') {
        next += 1;
    }
    let leading_zero_end = next;

    let mut has_zero_tail = true;

    // The integer part.
    while let Some(&c) = s.get(next) {
        let digit_value = digit_from_char(c);
        if digit_value >= base {
            break;
        }
        if mantissa_it != mantissa_last {
            fp_string.mantissa[mantissa_it] = digit_value;
            mantissa_it += 1;
        } else {
            has_zero_tail = has_zero_tail && digit_value == 0;
        }
        next += 1;
    }
    let whole_end = next;

    // The digits in the buffer before the point: "03333.111" 4, "00000.111" 0, "00000.001" -2.
    let mut exponent_adjustment = (whole_end - leading_zero_end) as i64;

    // [whole_end, dot_end): 0 or 1 '.'.
    if s.get(next) == Some(&b'.') {
        next += 1;
    }
    let dot_end = next;

    // Without a nonzero digit yet, the zeroes after the point go too, into the adjustment.
    if exponent_adjustment == 0 {
        while s.get(next) == Some(&b'0') {
            next += 1;
        }
        exponent_adjustment = dot_end as i64 - next as i64;
    }

    // The fractional part.
    while let Some(&c) = s.get(next) {
        let digit_value = digit_from_char(c);
        if digit_value >= base {
            break;
        }
        if mantissa_it != mantissa_last {
            fp_string.mantissa[mantissa_it] = digit_value;
            mantissa_it += 1;
        } else {
            has_zero_tail = has_zero_tail && digit_value == 0;
        }
        next += 1;
    }
    let frac_end = next;

    // At least one digit.
    if whole_begin == whole_end && dot_end == frac_end {
        return invalid_argument(0);
    }

    let exponent_prefix = if is_hexadecimal { b'p' } else { b'e' };

    let mut exponent_is_negative = false;
    let mut exp_abs_too_large = false;
    let mut exponent: i64 = 0;

    const MAXIMUM_TEMPORARY_DECIMAL_EXPONENT: i64 = 5200;
    const MINIMUM_TEMPORARY_DECIMAL_EXPONENT: i64 = -5200;

    // N4950 [charconv.from.chars]/6.3: no exponent for chars_format::fixed.
    if format != CharsFormat::Fixed && s.get(next).is_some_and(|&c| (c | 0x20) == exponent_prefix) {
        let mut unread = next + 1;

        if let Some(&sign @ (b'+' | b'-')) = s.get(unread) {
            exponent_is_negative = sign == b'-';
            unread += 1;
        }

        while let Some(&c) = s.get(unread) {
            let digit_value = digit_from_char(c);
            if digit_value >= 10 {
                break;
            }
            let digit_value = i64::from(digit_value);
            if exponent < i64::MAX / 10
                || (exponent == i64::MAX / 10 && digit_value <= i64::MAX % 10)
            {
                exponent = exponent * 10 + digit_value;
            } else {
                exp_abs_too_large = true;
            }
            unread += 1;
            // The exponent part is read.
            next = unread;
        }

        if exponent_is_negative {
            exponent = -exponent;
        }
    }

    // [frac_end, exponent_end): empty or "[EPep] sign[opt] digit-sequence".
    let exponent_end = next;

    // N4950 [charconv.from.chars]/6.2: chars_format::scientific needs the exponent.
    if format == CharsFormat::Scientific && frac_end == exponent_end {
        return invalid_argument(0);
    }

    // Trailing zeroes.
    while mantissa_it != 0 && fp_string.mantissa[mantissa_it - 1] == 0 {
        mantissa_it -= 1;
    }

    // All zeroes: zero, whatever the exponent (checked after the exponent, so next is past it).
    if mantissa_it == 0 {
        debug_assert!(has_zero_tail);
        *value = assemble_zero(fp_string.is_negative);
        return FromCharsResult {
            ptr: next,
            ec: None,
        };
    }

    // A hexadecimal exponent is binary and the adjustment counts hexits: 4 bits each.
    let exponent_adjustment_multiplier: i64 = if is_hexadecimal { 4 } else { 1 };

    if parser == Parser::Ucrt {
        // The UCRT's exponent (see Parser): at most 5201, checked before the adjustment and after.
        let out_of_range = FromCharsResult {
            ptr: next,
            ec: Some(Errc::ResultOutOfRange),
        };
        let read = if exp_abs_too_large {
            if exponent_is_negative { -5201 } else { 5201 }
        } else {
            exponent.clamp(-5201, 5201)
        };
        let total = read + exponent_adjustment * exponent_adjustment_multiplier;
        if read > MAXIMUM_TEMPORARY_DECIMAL_EXPONENT
            || (read >= MINIMUM_TEMPORARY_DECIMAL_EXPONENT
                && total > MAXIMUM_TEMPORARY_DECIMAL_EXPONENT)
        {
            *value = assemble_infinity(fp_string.is_negative);
            return out_of_range;
        }
        if read < MINIMUM_TEMPORARY_DECIMAL_EXPONENT || total < MINIMUM_TEMPORARY_DECIMAL_EXPONENT {
            *value = assemble_zero(fp_string.is_negative);
            return out_of_range;
        }
        fp_string.exponent = total as i32;
        fp_string.mantissa_count = mantissa_it as u32;
        // The UCRT does not note whether the digits it dropped were zero (see Parser).
        let (result, ec) = if is_hexadecimal {
            convert_hexadecimal_string(&fp_string, true)
        } else {
            convert_decimal_string(&fp_string, true, parser)
        };
        *value = result;
        return FromCharsResult { ptr: next, ec };
    }

    // An exponent of an overly large absolute value.
    if exp_abs_too_large {
        *value = if exponent > 0 {
            assemble_infinity(fp_string.is_negative)
        } else {
            assemble_zero(fp_string.is_negative)
        };
        return FromCharsResult {
            ptr: next,
            ec: Some(Errc::ResultOutOfRange),
        };
    }

    // Bring the exponent and the adjustment to the same sign, so neither overflows.
    if exponent > 0 && exponent_adjustment < 0 {
        if is_hexadecimal {
            let further_adjustment = (-((exponent - 1) / 4 + 1)).max(exponent_adjustment);
            exponent += further_adjustment * 4;
            exponent_adjustment -= further_adjustment;
        } else {
            let further_adjustment = (-exponent).max(exponent_adjustment);
            exponent += further_adjustment;
            exponent_adjustment -= further_adjustment;
        }
    } else if exponent < 0 && exponent_adjustment > 0 {
        if is_hexadecimal {
            let further_adjustment = ((-exponent - 1) / 4 + 1).min(exponent_adjustment);
            exponent += further_adjustment * 4;
            exponent_adjustment -= further_adjustment;
        } else {
            let further_adjustment = (-exponent).min(exponent_adjustment);
            exponent += further_adjustment;
            exponent_adjustment -= further_adjustment;
        }
    }

    if exponent > MAXIMUM_TEMPORARY_DECIMAL_EXPONENT
        || exponent_adjustment > MAXIMUM_TEMPORARY_DECIMAL_EXPONENT / exponent_adjustment_multiplier
    {
        // Overflow example: "1e+9999".
        *value = assemble_infinity(fp_string.is_negative);
        return FromCharsResult {
            ptr: next,
            ec: Some(Errc::ResultOutOfRange),
        };
    }

    if exponent < MINIMUM_TEMPORARY_DECIMAL_EXPONENT
        || exponent_adjustment < MINIMUM_TEMPORARY_DECIMAL_EXPONENT / exponent_adjustment_multiplier
    {
        // Underflow example: "1e-9999".
        *value = assemble_zero(fp_string.is_negative);
        return FromCharsResult {
            ptr: next,
            ec: Some(Errc::ResultOutOfRange),
        };
    }

    exponent += exponent_adjustment * exponent_adjustment_multiplier;

    if exponent > MAXIMUM_TEMPORARY_DECIMAL_EXPONENT {
        // Overflow example: "10e+5199".
        *value = assemble_infinity(fp_string.is_negative);
        return FromCharsResult {
            ptr: next,
            ec: Some(Errc::ResultOutOfRange),
        };
    }

    if exponent < MINIMUM_TEMPORARY_DECIMAL_EXPONENT {
        // Underflow example: "0.001e-5199".
        *value = assemble_zero(fp_string.is_negative);
        return FromCharsResult {
            ptr: next,
            ec: Some(Errc::ResultOutOfRange),
        };
    }

    fp_string.exponent = exponent as i32;
    fp_string.mantissa_count = mantissa_it as u32;

    let (result, ec) = if is_hexadecimal {
        convert_hexadecimal_string(&fp_string, has_zero_tail)
    } else {
        convert_decimal_string(&fp_string, has_zero_tail, parser)
    };
    *value = result;
    FromCharsResult { ptr: next, ec }
}

/// `_Starts_with_case_insensitive`: whether `s` starts with `lowercase`, ignoring case.
fn starts_with_case_insensitive(s: &[u8], lowercase: &[u8]) -> bool {
    s.len() >= lowercase.len() && s.iter().zip(lowercase).all(|(&c, &l)| (c | 0x20) == l)
}

/// `_Infinity_from_chars`: `s[next]` is an `i` of either case.
fn infinity_from_chars<T: FromCharsFloat>(
    s: &[u8],
    value: &mut T,
    minus_sign: bool,
    mut next: usize,
) -> FromCharsResult {
    if !starts_with_case_insensitive(&s[next + 1..], b"nf") {
        return invalid_argument(0);
    }
    next += 3;
    if starts_with_case_insensitive(&s[next..], b"inity") {
        next += 5;
    }
    *value = assemble_infinity(minus_sign);
    FromCharsResult {
        ptr: next,
        ec: None,
    }
}

/// What `nan` is followed by: `(ind)`, `(snan)`, another `(n-char-sequence)` or nothing more.
#[derive(Clone, Copy, PartialEq, Eq)]
enum NanSequence {
    None,
    Ind,
    Snan,
    Other,
}

/// After `nan` at `s[next..]`: the n-char-sequence in parentheses, if there is one, and where it
/// ends, as `_Nan_from_chars` reads it.
fn nan_sequence(s: &[u8], next: usize) -> (NanSequence, usize) {
    if s.get(next) != Some(&b'(') {
        return (NanSequence::None, next);
    }
    let seq_begin = next + 1;
    for (temp, &c) in s.iter().enumerate().skip(seq_begin) {
        if c == b')' {
            let sequence = &s[seq_begin..temp];
            let kind = if sequence.len() == 3 && starts_with_case_insensitive(sequence, b"ind") {
                NanSequence::Ind
            } else if sequence.len() == 4 && starts_with_case_insensitive(sequence, b"snan") {
                NanSequence::Snan
            } else {
                NanSequence::Other
            };
            return (kind, temp + 1);
        } else if c == b'_' || c.is_ascii_alphanumeric() {
            // Possibly nan(n-char-sequence[opt]): keep going.
        } else {
            break;
        }
    }
    (NanSequence::None, next)
}

/// `_Nan_from_chars`: `s[next]` is an `n` of either case.
fn nan_from_chars<T: FromCharsFloat>(
    s: &[u8],
    value: &mut T,
    mut minus_sign: bool,
    next: usize,
) -> FromCharsResult {
    if !starts_with_case_insensitive(&s[next + 1..], b"an") {
        return invalid_argument(0);
    }
    let (sequence, next) = nan_sequence(s, next + 3);

    let mut quiet = true;
    match sequence {
        // The UCRT's indeterminate NaN is the negative quiet NaN without payload: "nan(ind)" and
        // "-nan(ind)" are the same.
        NanSequence::Ind => minus_sign = true,
        NanSequence::Snan => quiet = false,
        NanSequence::None | NanSequence::Other => {}
    }

    // Unlike strtod, plain "nan" is numeric_limits::quiet_NaN(), without payload bits.
    let mut uint_value = T::SHIFTED_EXPONENT_MASK;
    if minus_sign {
        uint_value |= T::SHIFTED_SIGN_MASK;
    }
    if quiet {
        uint_value |= T::SPECIAL_NAN_MANTISSA_MASK;
    } else {
        uint_value |= 1;
    }
    *value = T::from_uint(uint_value);
    FromCharsResult {
        ptr: next,
        ec: None,
    }
}

/// `std::from_chars(first, last, value, format)` for `float` and `double`: an optional `-`, then
/// a number in `format` (hexadecimal without `0x`), `inf`, `infinity`, `nan` or `nan(...)`.
pub fn from_chars_float<T: FromCharsFloat>(
    s: &[u8],
    value: &mut T,
    format: CharsFormat,
) -> FromCharsResult {
    let mut minus_sign = false;
    let mut next = 0;

    if s.is_empty() {
        return invalid_argument(0);
    }

    if s[next] == b'-' {
        minus_sign = true;
        next += 1;
        if next == s.len() {
            return invalid_argument(0);
        }
    }

    // One test tells numbers ('.', digits and a to f, folded) from inf and nan.
    let folded_start = s[next] | 0x20;
    if folded_start <= b'f' {
        ordinary_floating_from_chars(s, value, format, minus_sign, next, Parser::Stl)
    } else if folded_start == b'i' {
        infinity_from_chars(s, value, minus_sign, next)
    } else if folded_start == b'n' {
        nan_from_chars(s, value, minus_sign, next)
    } else {
        invalid_argument(0)
    }
}

/// The `errno` a C conversion function sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Errno {
    /// `ERANGE`: the value overflowed (or, for `strtod` and `strtof`, rounded to zero).
    Range,
    /// `EINVAL`: an invalid base; the UCRT calls its invalid parameter handler, which ends the
    /// program unless one was set that returns.
    Invalid,
}

/// What a C `strto*` function gives: the value, the bytes read (`*endptr - nptr`, 0 when nothing
/// was converted) and the `errno` it set (`None`: left as it was).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strto<T> {
    /// The return value.
    pub value: T,
    /// `*endptr - nptr`.
    pub end: usize,
    /// The `errno` set.
    pub errno: Option<Errno>,
}

/// The bytes of a C string: up to the first NUL.
fn c_string(s: &[u8]) -> &[u8] {
    match s.iter().position(|&c| c == 0) {
        Some(n) => &s[..n],
        None => s,
    }
}

/// `isspace` of the "C" locale: space, `\t`, `\n`, `\v`, `\f`, `\r`.
const fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// The white space and the sign at the start: where the subject sequence starts and whether it
/// is negative.
fn space_and_sign(s: &[u8]) -> (usize, bool) {
    let mut i = s.iter().take_while(|&&c| is_space(c)).count();
    let negative = match s.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    (i, negative)
}

/// The UCRT's reading of what follows `nan` at `s[next..]`. After the `(` it matches `snan)`, then
/// `ind)`, character by character and in either case, the second match going on from where the
/// first one stopped; otherwise it reads letters, digits and `_` up to a `)`. So `nan(sind)`,
/// whose `s` begins `snan)`, is `nan(ind)`, where `from_chars` sees another n-char-sequence.
fn ucrt_nan_sequence(s: &[u8], next: usize) -> (NanSequence, usize) {
    if s.get(next) != Some(&b'(') {
        return (NanSequence::None, next);
    }
    let match_word = |i: &mut usize, word: &[u8]| {
        for &w in word {
            match s.get(*i) {
                Some(&c) if c.to_ascii_lowercase() == w => *i += 1,
                _ => return false,
            }
        }
        true
    };
    let mut i = next + 1;
    if match_word(&mut i, b"snan)") {
        return (NanSequence::Snan, i);
    }
    if match_word(&mut i, b"ind)") {
        return (NanSequence::Ind, i);
    }
    while let Some(&c) = s.get(i) {
        if c == b')' {
            return (NanSequence::Other, i + 1);
        }
        if c != b'_' && !c.is_ascii_alphanumeric() {
            break;
        }
        i += 1;
    }
    (NanSequence::None, next)
}

fn strto_float<T: FromCharsFloat>(s: &[u8]) -> Strto<T> {
    let s = c_string(s);
    let (start, negative) = space_and_sign(s);
    let no_conversion = Strto {
        value: T::zero(),
        end: 0,
        errno: None,
    };
    let sign = if negative { T::SHIFTED_SIGN_MASK } else { 0 };
    let rest = &s[start..];

    if starts_with_case_insensitive(rest, b"inf") {
        let end = if starts_with_case_insensitive(&rest[3..], b"inity") {
            8
        } else {
            3
        };
        return Strto {
            value: T::from_uint(sign | T::SHIFTED_EXPONENT_MASK),
            end: start + end,
            errno: None,
        };
    }

    if starts_with_case_insensitive(rest, b"nan") {
        let (sequence, end) = ucrt_nan_sequence(rest, 3);
        let bits = match sequence {
            // Negative whatever the sign, as in from_chars.
            NanSequence::Ind => {
                T::SHIFTED_SIGN_MASK | T::SHIFTED_EXPONENT_MASK | T::SPECIAL_NAN_MANTISSA_MASK
            }
            NanSequence::Snan => sign | T::SHIFTED_EXPONENT_MASK | 1,
            // The UCRT's plain NaN: quiet, every payload bit set.
            NanSequence::None | NanSequence::Other => {
                sign | T::SHIFTED_EXPONENT_MASK | T::DENORMAL_MANTISSA_MASK
            }
        };
        return Strto {
            value: T::from_uint(bits),
            end: start + end,
            errno: None,
        };
    }

    let mut value = T::zero();
    let hex = rest.len() >= 2 && rest[0] == b'0' && (rest[1] | 0x20) == b'x';
    let (result, offset) = if hex {
        let result = ordinary_floating_from_chars(
            &rest[2..],
            &mut value,
            CharsFormat::Hex,
            negative,
            0,
            Parser::Ucrt,
        );
        if result.ec == Some(Errc::InvalidArgument) {
            // "0x" without a hex digit: the subject sequence is the "0".
            return Strto {
                value: assemble_zero(negative),
                end: start + 1,
                errno: None,
            };
        }
        (result, start + 2)
    } else {
        let result = ordinary_floating_from_chars(
            rest,
            &mut value,
            CharsFormat::General,
            negative,
            0,
            Parser::Ucrt,
        );
        if result.ec == Some(Errc::InvalidArgument) {
            return no_conversion;
        }
        (result, start)
    };
    let errno = (result.ec == Some(Errc::ResultOutOfRange)).then_some(Errno::Range);
    Strto {
        value,
        end: offset + result.ptr,
        errno,
    }
}

/// `strtod`.
#[must_use]
pub fn strtod(s: &[u8]) -> Strto<f64> {
    strto_float(s)
}

/// `strtof`: parsed to `float` directly, not through `double`.
#[must_use]
pub fn strtof(s: &[u8]) -> Strto<f32> {
    strto_float(s)
}

/// `atof`: [`strtod`]'s value.
#[must_use]
pub fn atof(s: &[u8]) -> f64 {
    strtod(s).value
}

/// The `strto*` integer conversions on a type of `bits` bits: the value's bits.
fn strto_integer(s: &[u8], base: i32, signed: bool, bits: u32) -> Strto<u64> {
    let no_conversion = Strto {
        value: 0,
        end: 0,
        errno: None,
    };
    if base != 0 && !(2..=36).contains(&base) {
        return Strto {
            errno: Some(Errno::Invalid),
            ..no_conversion
        };
    }
    let s = c_string(s);
    let (mut i, negative) = space_and_sign(s);

    let mut base = base as u32;
    if (base == 0 || base == 16)
        && s.get(i) == Some(&b'0')
        && s.get(i + 1).is_some_and(|&c| (c | 0x20) == b'x')
    {
        // The prefix; a digit must follow it, or nothing is converted.
        i += 2;
        base = 16;
    } else if base == 0 {
        base = if s.get(i) == Some(&b'0') { 8 } else { 10 };
    }
    let base = u64::from(base);

    let uint_max = uint_max(bits);
    let (risky_val, max_digit) = (uint_max / base, uint_max % base);
    let first_digit = i;
    let mut number: u64 = 0;
    let mut overflow = false;
    while let Some(&c) = s.get(i) {
        let digit = u64::from(digit_from_char(c));
        if digit >= base {
            break;
        }
        if number < risky_val || (number == risky_val && digit <= max_digit) {
            number = number * base + digit;
        } else {
            overflow = true;
        }
        i += 1;
    }
    if i == first_digit {
        return no_conversion;
    }

    let int_max = uint_max >> 1;
    let (value, errno) = if signed {
        let limit = if negative { int_max + 1 } else { int_max };
        if overflow || number > limit {
            let saturated = if negative {
                (int_max + 1) & uint_max
            } else {
                int_max
            };
            (saturated, Some(Errno::Range))
        } else if negative {
            (number.wrapping_neg() & uint_max, None)
        } else {
            (number, None)
        }
    } else if overflow {
        (uint_max, Some(Errno::Range))
    } else if negative {
        (number.wrapping_neg() & uint_max, None)
    } else {
        (number, None)
    };
    Strto {
        value,
        end: i,
        errno,
    }
}

/// `strtol` (`long` is 32 bits).
#[must_use]
pub fn strtol(s: &[u8], base: i32) -> Strto<i32> {
    let r = strto_integer(s, base, true, 32);
    Strto {
        value: r.value as u32 as i32,
        end: r.end,
        errno: r.errno,
    }
}

/// `strtoul`.
#[must_use]
pub fn strtoul(s: &[u8], base: i32) -> Strto<u32> {
    let r = strto_integer(s, base, false, 32);
    Strto {
        value: r.value as u32,
        end: r.end,
        errno: r.errno,
    }
}

/// `strtoll`.
#[must_use]
pub fn strtoll(s: &[u8], base: i32) -> Strto<i64> {
    let r = strto_integer(s, base, true, 64);
    Strto {
        value: r.value as i64,
        end: r.end,
        errno: r.errno,
    }
}

/// `strtoull`.
#[must_use]
pub fn strtoull(s: &[u8], base: i32) -> Strto<u64> {
    strto_integer(s, base, false, 64)
}

/// `atoi`: [`strtol`] in base 10.
#[must_use]
pub fn atoi(s: &[u8]) -> i32 {
    strtol(s, 10).value
}

/// `atol`: [`strtol`] in base 10.
#[must_use]
pub fn atol(s: &[u8]) -> i32 {
    strtol(s, 10).value
}

/// `atoll`: [`strtoll`] in base 10.
#[must_use]
pub fn atoll(s: &[u8]) -> i64 {
    strtoll(s, 10).value
}

/// The exception a `std::sto*` function throws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoError {
    /// `std::invalid_argument`: no number at the start (or, with an invalid base, the UCRT's
    /// invalid parameter returned).
    InvalidArgument,
    /// `std::out_of_range`: the C function set `ERANGE`.
    OutOfRange,
}

impl std::fmt::Display for StoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            StoError::InvalidArgument => "invalid sto* argument",
            StoError::OutOfRange => "sto* argument out of range",
        })
    }
}

impl std::error::Error for StoError {}

/// The STL's `sto*` around a C conversion: the value and `*idx`.
fn sto<T>(result: Strto<T>) -> Result<(T, usize), StoError> {
    if result.end == 0 {
        return Err(StoError::InvalidArgument);
    }
    if result.errno == Some(Errno::Range) {
        return Err(StoError::OutOfRange);
    }
    Ok((result.value, result.end))
}

/// `std::stoi(str, &idx, base)`: [`strtol`], whose `long` is an `int` with MSVC.
///
/// # Errors
///
/// [`StoError`] as the C++ function throws.
pub fn stoi(s: impl AsRef<[u8]>, base: i32) -> Result<(i32, usize), StoError> {
    sto(strtol(s.as_ref(), base))
}

/// `std::stol(str, &idx, base)`: [`strtol`].
///
/// # Errors
///
/// [`StoError`] as the C++ function throws.
pub fn stol(s: impl AsRef<[u8]>, base: i32) -> Result<(i32, usize), StoError> {
    sto(strtol(s.as_ref(), base))
}

/// `std::stoul(str, &idx, base)`: [`strtoul`].
///
/// # Errors
///
/// [`StoError`] as the C++ function throws.
pub fn stoul(s: impl AsRef<[u8]>, base: i32) -> Result<(u32, usize), StoError> {
    sto(strtoul(s.as_ref(), base))
}

/// `std::stoll(str, &idx, base)`: [`strtoll`].
///
/// # Errors
///
/// [`StoError`] as the C++ function throws.
pub fn stoll(s: impl AsRef<[u8]>, base: i32) -> Result<(i64, usize), StoError> {
    sto(strtoll(s.as_ref(), base))
}

/// `std::stoull(str, &idx, base)`: [`strtoull`].
///
/// # Errors
///
/// [`StoError`] as the C++ function throws.
pub fn stoull(s: impl AsRef<[u8]>, base: i32) -> Result<(u64, usize), StoError> {
    sto(strtoull(s.as_ref(), base))
}

/// `std::stof(str, &idx)`: [`strtof`].
///
/// # Errors
///
/// [`StoError`] as the C++ function throws.
pub fn stof(s: impl AsRef<[u8]>) -> Result<(f32, usize), StoError> {
    sto(strtof(s.as_ref()))
}

/// `std::stod(str, &idx)` (and `std::stold`, `long double` being `double`): [`strtod`].
///
/// # Errors
///
/// [`StoError`] as the C++ function throws.
pub fn stod(s: impl AsRef<[u8]>) -> Result<(f64, usize), StoError> {
    sto(strtod(s.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f64_from(s: &str, format: CharsFormat) -> (u64, FromCharsResult) {
        let mut v = f64::from_bits(0xDEAD_BEEF);
        let r = from_chars_float(s.as_bytes(), &mut v, format);
        (v.to_bits(), r)
    }

    fn ok(ptr: usize) -> FromCharsResult {
        FromCharsResult { ptr, ec: None }
    }

    #[test]
    fn integers() {
        let mut v = 7i32;
        assert_eq!(from_chars_int(b"-2147483648x", &mut v, 10), ok(11));
        assert_eq!(v, i32::MIN);
        assert_eq!(
            from_chars_int(b"2147483648", &mut v, 10),
            FromCharsResult {
                ptr: 10,
                ec: Some(Errc::ResultOutOfRange)
            }
        );
        assert_eq!(v, i32::MIN);
        assert_eq!(from_chars_int(b"-", &mut v, 10), invalid_argument(0));
        assert_eq!(from_chars_int(b"+1", &mut v, 10), invalid_argument(0));
        let mut u = 0u8;
        assert_eq!(from_chars_int(b"-1", &mut u, 10), invalid_argument(0));
        assert_eq!(
            from_chars_int(b"zZ", &mut u, 36),
            FromCharsResult {
                ptr: 2,
                ec: Some(Errc::ResultOutOfRange)
            }
        );
        assert_eq!(from_chars_int(b"73", &mut u, 36), ok(2));
        assert_eq!(u, 7 * 36 + 3);
        let mut w = 0u64;
        assert_eq!(from_chars_int(b"ffffffffffffffff", &mut w, 16), ok(16));
        assert_eq!(w, u64::MAX);
    }

    #[test]
    fn floats() {
        assert_eq!(
            f64_from("0.1", CharsFormat::General),
            (0.1f64.to_bits(), ok(3))
        );
        assert_eq!(
            f64_from("-1e-400", CharsFormat::General),
            (
                0x8000_0000_0000_0000,
                FromCharsResult {
                    ptr: 7,
                    ec: Some(Errc::ResultOutOfRange)
                }
            )
        );
        assert_eq!(
            f64_from("1e400x", CharsFormat::General),
            (
                0x7FF0_0000_0000_0000,
                FromCharsResult {
                    ptr: 5,
                    ec: Some(Errc::ResultOutOfRange)
                }
            )
        );
        assert_eq!(
            f64_from("1.8p1", CharsFormat::Hex),
            (3.0f64.to_bits(), ok(5))
        );
        assert_eq!(
            f64_from("1e5", CharsFormat::Fixed),
            (1.0f64.to_bits(), ok(1))
        );
        assert_eq!(
            f64_from("15", CharsFormat::Scientific),
            (0xDEAD_BEEF, invalid_argument(0))
        );
        assert_eq!(
            f64_from("2.4703282292062328e-324", CharsFormat::General).0,
            1
        );
        assert_eq!(
            f64_from("2.4703282292062327e-324", CharsFormat::General).0,
            0
        );
        assert_eq!(
            f64_from("InFiNiTyX", CharsFormat::General),
            (0x7FF0_0000_0000_0000, ok(8))
        );
        assert_eq!(
            f64_from("-nan(ind)", CharsFormat::General),
            (0xFFF8_0000_0000_0000, ok(9))
        );
        assert_eq!(
            f64_from("nan(ind)", CharsFormat::General),
            (0xFFF8_0000_0000_0000, ok(8))
        );
        assert_eq!(
            f64_from("nan(snan)", CharsFormat::General),
            (0x7FF0_0000_0000_0001, ok(9))
        );
        assert_eq!(
            f64_from("nan(1_a)", CharsFormat::General),
            (0x7FF8_0000_0000_0000, ok(8))
        );
        assert_eq!(
            f64_from("nan(1-a)", CharsFormat::General),
            (0x7FF8_0000_0000_0000, ok(3))
        );
        // Ties to even, and a tie broken by a digit past the 768th.
        let tie = format!("9007199254740993.{}", "0".repeat(800));
        assert_eq!(
            f64_from(&tie, CharsFormat::General).0,
            9007199254740992f64.to_bits()
        );
        let above = format!("9007199254740993.{}1", "0".repeat(800));
        assert_eq!(
            f64_from(&above, CharsFormat::General).0,
            9007199254740994f64.to_bits()
        );
        let mut f = 0f32;
        assert_eq!(
            from_chars_float(b"3.4028235e38", &mut f, CharsFormat::General),
            ok(12)
        );
        assert_eq!(f, f32::MAX);
        assert_eq!(
            from_chars_float(b"3.4028236e38", &mut f, CharsFormat::General),
            FromCharsResult {
                ptr: 12,
                ec: Some(Errc::ResultOutOfRange)
            }
        );
        assert_eq!(f, f32::INFINITY);
    }

    #[test]
    fn strtod_and_strtol() {
        let d = |s: &str| {
            let r = strtod(s.as_bytes());
            (r.value.to_bits(), r.end, r.errno)
        };
        assert_eq!(d(" \t+1.5e1z"), (15f64.to_bits(), 8, None));
        assert_eq!(d("+-1"), (0, 0, None));
        assert_eq!(d("0x"), (0, 1, None));
        assert_eq!(d("-0x.p1"), (0x8000_0000_0000_0000, 2, None));
        assert_eq!(d("0x1p"), (1f64.to_bits(), 3, None));
        assert_eq!(d("infin"), (0x7FF0_0000_0000_0000, 3, None));
        assert_eq!(d("nan"), (0x7FFF_FFFF_FFFF_FFFF, 3, None));
        assert_eq!(d("-NaN(x)"), (0xFFFF_FFFF_FFFF_FFFF, 7, None));
        assert_eq!(d("nan(IND)"), (0xFFF8_0000_0000_0000, 8, None));
        assert_eq!(d("-1e999"), (0xFFF0_0000_0000_0000, 6, Some(Errno::Range)));
        assert_eq!(d("1e-999"), (0, 6, Some(Errno::Range)));
        assert_eq!(d("4e-324"), (1, 6, None));
        assert_eq!(d("1\x002"), (1f64.to_bits(), 1, None));
        assert_eq!(strtof(b"nan(snan)").value.to_bits(), 0x7F80_0001);

        let l = |s: &str, base| {
            let r = strtol(s.as_bytes(), base);
            (r.value, r.end, r.errno)
        };
        assert_eq!(l(" -0x1Fg", 0), (-31, 6, None));
        assert_eq!(l("0x", 16), (0, 0, None));
        assert_eq!(l("0xg", 0), (0, 0, None));
        assert_eq!(l("08", 0), (0, 1, None));
        assert_eq!(l("2147483648", 10), (i32::MAX, 10, Some(Errno::Range)));
        assert_eq!(l("-2147483649", 10), (i32::MIN, 11, Some(Errno::Range)));
        assert_eq!(l("1", 37), (0, 0, Some(Errno::Invalid)));
        let r = strtoul(b"-1", 10);
        assert_eq!((r.value, r.errno), (u32::MAX, None));
        let r = strtoul(b"-4294967296", 10);
        assert_eq!((r.value, r.errno), (u32::MAX, Some(Errno::Range)));
        assert_eq!(strtoull(b"0x10", 0).value, 16);
    }

    #[test]
    fn sto_functions() {
        assert_eq!(stoi("  42abc", 10), Ok((42, 4)));
        assert_eq!(stoi("abc", 10), Err(StoError::InvalidArgument));
        assert_eq!(stoi("99999999999", 10), Err(StoError::OutOfRange));
        assert_eq!(stod("1e-400"), Err(StoError::OutOfRange));
        assert_eq!(stod("4e-324"), Ok((5e-324, 6)));
        assert_eq!(stof(String::from("0.5")), Ok((0.5, 3)));
        assert_eq!(stoull("-1", 10), Ok((u64::MAX, 2)));
    }
}
