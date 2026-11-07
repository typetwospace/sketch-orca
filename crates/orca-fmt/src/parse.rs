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
// and its operations, _Divide among them, _Assemble_floating_point_zero,
// _Assemble_floating_point_infinity, _Right_shift_with_rounding,
// _Assemble_floating_point_value_no_shift, _Assemble_floating_point_value,
// _Assemble_floating_point_value_from_big_integer_flt,
// _Accumulate_decimal_digits_into_big_integer_flt, _Convert_decimal_string_to_floating_type,
// _Convert_hexadecimal_string_to_floating_type, _Ordinary_floating_from_chars,
// _Infinity_from_chars, _Nan_from_chars and _Floating_from_chars) and stl/inc/xbit_ops.h
// (_Bit_scan_reverse), and stoi, stol, stoul, stoll, stoull, stof and stod from stl/inc/string, of
// microsoft/STL at the tag vs-2022-17.12 (commit 91e425587a0b9b5ac0d1005844c661fee2e2813b), the
// headers of MSVC 14.42, function for function, on 2026-11-06. Its copyright holder is Microsoft
// Corporation, and it is under Apache-2.0 WITH LLVM-exception (licenses/Apache-2.0.txt and
// licenses/LLVM-exception.txt). The STL marks its floating-point parser as derived from the UCRT's
// corecrt_internal_big_integer.h and corecrt_internal_strtox.h; Microsoft publishes it, in the STL,
// under that licence.
//
// strtod, strtof, the strtol family and atof, atoi, atol and atoll are not the UCRT's code, whose
// source is not under an open licence. They are written from the C standard (C17 7.22.1) around
// the STL's parser. Where the UCRT reads a number otherwise than the STL (Rules) or the standard
// leaves the choice to it (its NaNs: ucrt_nan_sequence and ucrt_nan), they follow black-box tests
// of the UCRT's own functions: tests/fmt.rs calls them and compares (the ucrt_ tests pin each of
// these), and parity/probes/fmt.sop1 records what the C++ engine's build of them does.
//
// Provenance. The first version of this file (2026-11-06) took the UCRT's three rounding
// differences and its NaN spellings from a reading of the UCRT's source in the Windows SDK, which
// CONTRIBUTING.md does not allow. On 2026-11-06 that code was taken out, without being studied, and
// written again by someone who had not read the UCRT's source: the differences and the NaN
// spellings were found with black-box tests of the UCRT alone, and _Divide,
// _Convert_decimal_string_to_floating_type and _Ordinary_floating_from_chars, where they sit, were
// translated again from the STL's charconv of MSVC 14.42. The earlier version is still in this
// repository's history (README.md, open items).
// CONTRIBUTING.md, "Behaviour without source", gives the rule.

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
//!   any case. The number is the STL's parser above, read as the UCRT reads it (the private
//!   `Rules` enum): digits after the 768th ignored; an exponent above 5200 or below -5200
//!   infinite or zero before the digits' place is added; and a tie rounded up, not to even, when
//!   its fractional part is divided by a one-element denominator with a numerator of two or more
//!   (eight or nine decimal places). The NaNs: `nan` and `nan(...)` give the quiet NaN with every
//!   payload bit set, `nan(snan)` the signalling NaN with payload 1, and `nan(ind)`, also with
//!   `s`, `sn`, `sna` or `snan` before the `ind`, the negative quiet NaN without payload, whatever
//!   the sign. `0x` without a hex digit reads as the `0`; `ERANGE` for an overflow (to an infinity)
//!   and for a result that rounds to zero, not for one that is merely subnormal.
//! - [`strtol`], [`strtoul`], [`strtoll`], [`strtoull`]: `long` is 32 bits. Base 0 reads `0x` as
//!   hexadecimal and a leading `0` as octal; with base 0 or 16, a `0x` without a hex digit after it
//!   is no number at all (the UCRT's choice, which `tests/fmt.rs` compares); an overflow gives the
//!   limit and `ERANGE`; `strtoul` and `strtoull` negate a `-` number modulo 2^N. A base other than
//!   0 and 2 to 36 is `EINVAL` (the UCRT's invalid parameter).
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

/// Whose reading of a decimal or hexadecimal number a parse follows. `std::from_chars` is the
/// STL's (`Stl`). The UCRT's `strtod` and `strtof` (`Ucrt`) read three things otherwise; each was
/// found by black-box tests of the UCRT's own functions, which `tests/fmt.rs` keeps (the `ucrt_`
/// tests), never from the UCRT's source:
///
/// - A significant digit after the 768th (decimal or hexadecimal) is ignored: it neither adds to
///   the value nor puts the value above a tie, so a tie with a 1 as its 769th digit rounds as the
///   tie does. The STL notes such a digit as a tail that is not zero.
/// - An exponent above 5200 gives an infinity and one below -5200 a zero (`ERANGE` both), before
///   the digits' place is counted: `0.<5300 zeros>1e5301` is infinite, `1<5300 zeros>e-5301` zero.
///   The STL adds the digits' place first.
/// - A tie rounds up (away from zero), not to even, when its fractional part is divided by a
///   denominator of one 32-bit element (at most nine fraction digits) with a numerator of two or
///   more: the UCRT then takes the remainder for one that is not zero. These are the double ties
///   with eight or nine decimal places and the float ties with eight or nine, whose fraction is
///   not too small (`strtod("39902590343405.22265625")` and `strtof("52159.38671875")` round up).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rules {
    /// `std::from_chars`.
    Stl,
    /// `strtod` and `strtof`.
    Ucrt,
}

/// `_Divide`, which the STL translated from .NET's `BigIntegerBuilder.ModDivCore`: the low 64 bits
/// of `numerator / denominator`, the remainder left in `numerator`, trimmed. The limbs above the
/// remainder are set to zero again.
fn divide(numerator: &mut BigInteger, denominator: &BigInteger) -> u64 {
    let quotient = divide_core(numerator, denominator);
    let used = numerator.used as usize;
    numerator.data[used..].fill(0);
    quotient
}

/// The body of `_Divide`.
fn divide_core(numerator: &mut BigInteger, denominator: &BigInteger) -> u64 {
    // A zero numerator: the quotient and the remainder are zero.
    if numerator.used == 0 {
        return 0;
    }

    debug_assert!(denominator.used != 0, "division by zero");

    let mut max_numerator_element_index = numerator.used - 1;
    let max_denominator_element_index = denominator.used - 1;

    // A denominator of one element: the fast route.
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

        // Counting down, the last assignment to the quotient is the right one.
        let mut quotient: u64 = 0;
        let mut uu: u64 = 0;
        for iv in (0..=max_numerator_element_index as usize).rev() {
            uu = (uu << 32) | u64::from(numerator.data[iv]);
            quotient = (quotient << 32)
                .wrapping_add(u64::from((uu / u64::from(small_denominator)) as u32));
            uu %= u64::from(small_denominator);
        }

        numerator.data[1] = (uu >> 32) as u32;
        numerator.data[0] = uu as u32;

        numerator.used = if numerator.data[1] > 0 {
            2
        } else if numerator.data[0] > 0 {
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

    // Whether the quotient has cu_diff or cu_diff + 1 elements.
    let mut cu_quo = cu_diff;
    let mut iu = max_numerator_element_index as i32;
    loop {
        if iu < cu_diff {
            cu_quo += 1;
            break;
        }

        let den = denominator.data[(iu - cu_diff) as usize];
        let num = numerator.data[iu as usize];
        if den != num {
            if den < num {
                cu_quo += 1;
            }
            break;
        }
        iu -= 1;
    }

    if cu_quo == 0 {
        return 0;
    }

    // The limbs for the trial divisions, normalised so the high bit is set.
    let mut u_den = denominator.data[(cu_den - 1) as usize];
    let mut u_den_next = denominator.data[(cu_den - 2) as usize];

    let cbit_shift_left = count_sequential_high_zeroes(u_den);
    let cbit_shift_right = 32 - cbit_shift_left;
    if cbit_shift_left > 0 {
        u_den = (u_den << cbit_shift_left) | (u_den_next >> cbit_shift_right);
        u_den_next <<= cbit_shift_left;

        if cu_den > 2 {
            u_den_next |= denominator.data[(cu_den - 3) as usize] >> cbit_shift_right;
        }
    }

    let mut quotient: u64 = 0;
    for iu in (0..cu_quo as u32).rev() {
        // The high (normalised) bits of the numerator.
        let u_num_hi = if iu + cu_den <= max_numerator_element_index {
            numerator.data[(iu + cu_den) as usize]
        } else {
            0
        };

        let mut uu_num =
            (u64::from(u_num_hi) << 32) | u64::from(numerator.data[(iu + cu_den - 1) as usize]);

        let mut u_num_next = numerator.data[(iu + cu_den - 2) as usize];
        if cbit_shift_left > 0 {
            uu_num = (uu_num << cbit_shift_left) | u64::from(u_num_next >> cbit_shift_right);
            u_num_next <<= cbit_shift_left;

            if iu + cu_den >= 3 {
                u_num_next |= numerator.data[(iu + cu_den - 3) as usize] >> cbit_shift_right;
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

        // Multiply and subtract; the digit may be one too large, and a borrow at the end adds the
        // denominator back and takes one from the digit.
        if uu_quo > 0 {
            let mut uu_borrow: u64 = 0;

            for iu2 in 0..cu_den {
                uu_borrow = uu_borrow
                    .wrapping_add(uu_quo.wrapping_mul(u64::from(denominator.data[iu2 as usize])));

                let u_sub = uu_borrow as u32;
                uu_borrow >>= 32;
                let ix = (iu + iu2) as usize;
                if numerator.data[ix] < u_sub {
                    uu_borrow += 1;
                }

                numerator.data[ix] = numerator.data[ix].wrapping_sub(u_sub);
            }

            if u64::from(u_num_hi) < uu_borrow {
                // Add, with the carry.
                let mut u_carry: u32 = 0;
                for iu2 in 0..cu_den {
                    let ix = (iu + iu2) as usize;
                    let sum = u64::from(numerator.data[ix])
                        + u64::from(denominator.data[iu2 as usize])
                        + u64::from(u_carry);

                    numerator.data[ix] = sum as u32;
                    u_carry = (sum >> 32) as u32;
                }

                uu_quo -= 1;
            }

            max_numerator_element_index = iu + cu_den - 1;
        }

        quotient = (quotient << 32).wrapping_add(u64::from(uu_quo as u32));
    }

    // The remainder, trimmed.
    let mut used = max_numerator_element_index + 1;
    while used != 0 && numerator.data[(used - 1) as usize] == 0 {
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

/// `_Convert_decimal_string_to_floating_type`: `[sign] 0.mantissa * 10^exponent` as `T`, with the
/// UCRT's reading of a remainder after the division under [`Rules::Ucrt`].
fn convert_decimal_string<T: FromCharsFloat>(
    data: &FloatingPointString,
    mut has_zero_tail: bool,
    rules: Rules,
) -> (T, Option<Errc>) {
    // An N-bit mantissa takes N + 1 bits of precision, the last for rounding (fewer are fine: then
    // there is nothing to round).
    let required_bits_of_precision = (T::MANTISSA_BITS + 1) as u32;

    // The input is 0.mantissa * 10^exponent. Its integer part is the first `exponent` digits, or
    // all of them if there are fewer; with a zero or negative exponent it is empty. The other
    // digits are the fractional part.
    let positive_exponent = crate::cpp_max(0, data.exponent) as u32;
    let integer_digits_present = positive_exponent.min(data.mantissa_count);
    let integer_digits_missing = positive_exponent - integer_digits_present;
    let digits = &data.mantissa[..data.mantissa_count as usize];
    let (integer_digits, fractional_digits) = digits.split_at(integer_digits_present as usize);
    let fractional_digits_present = fractional_digits.len() as u32;

    // The integer part as a big integer.
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

    // With more bits than the precision needs, or no fractional part, the integer part is the
    // value.
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

    // Otherwise the fractional part gives the remaining bits: as a fraction N/M, N its digits and M
    // the power of ten that makes N/M its value.
    let mut fractional_numerator = BigInteger::new();
    accumulate_decimal_digits(fractional_digits, &mut fractional_numerator);

    let fractional_denominator_exponent = if data.exponent < 0 {
        fractional_digits_present + data.exponent.unsigned_abs()
    } else {
        fractional_digits_present
    };

    let mut fractional_denominator = BigInteger::one();
    if !multiply_by_power_of_ten(&mut fractional_denominator, fractional_denominator_exponent) {
        // With integer digits the exponent cannot be this small: a true underflow.
        // Underflow example: "1e-2000".
        return (
            assemble_zero(data.is_negative),
            Some(Errc::ResultOutOfRange),
        );
    }

    // The numerator is below the denominator. Its high bit is moved to the denominator's, so that
    // shifting it N bits further left gives N bits of precision.
    let fractional_numerator_bits = bit_scan_reverse_big(&fractional_numerator);
    let fractional_denominator_bits = bit_scan_reverse_big(&fractional_denominator);

    // `denominator_bits > numerator_bits ? denominator_bits - numerator_bits : 0`.
    let fractional_shift = fractional_denominator_bits.saturating_sub(fractional_numerator_bits);

    if fractional_shift > 0 {
        let shifted = shift_left(&mut fractional_numerator, fractional_shift);
        debug_assert!(shifted);
    }

    let required_fractional_bits_of_precision =
        required_bits_of_precision - integer_bits_of_precision;

    let mut remaining_bits_of_precision_required = required_fractional_bits_of_precision;
    if integer_bits_of_precision > 0 {
        // A fractional part whose digits start too far right to give a bit or to change the
        // rounding leaves the integer part's bits as they are (5.0000000000000000000001). At a
        // shift equal to the bits needed, it still decides the rounding (a large odd integer and
        // .5 or more), so the division is done.
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

    // Without an integer part, the exponent comes from the fractional part: the power of two that
    // brings it into [1.0, 2.0), the shift above or one more.
    let fractional_exponent = if fractional_numerator.less_than(&fractional_denominator) {
        fractional_shift + 1
    } else {
        fractional_shift
    };

    // The STL only checks this shift in its own test builds (`_STL_INTERNAL_CHECK`). It overflows
    // for a value far below the smallest subnormal one, whose denominator already fills the big
    // integer: the numerator is then zero, and the value rounds to zero with a range error.
    let _ = shift_left(
        &mut fractional_numerator,
        remaining_bits_of_precision_required,
    );

    // Rules::Ucrt: a one-element denominator and a numerator of two or more elements leave a
    // remainder the UCRT takes for one that is not zero.
    let ucrt_remainder_not_zero =
        rules == Rules::Ucrt && fractional_denominator.used == 1 && fractional_numerator.used >= 2;

    let mut fractional_mantissa = divide(&mut fractional_numerator, &fractional_denominator);

    has_zero_tail = has_zero_tail && fractional_numerator.used == 0 && !ucrt_remainder_not_zero;

    // Drop any bits beyond the precision needed.
    let fractional_mantissa_bits = bit_scan_reverse_64(fractional_mantissa);
    if fractional_mantissa_bits > required_fractional_bits_of_precision {
        let shift = fractional_mantissa_bits - required_fractional_bits_of_precision;
        has_zero_tail = has_zero_tail && (fractional_mantissa & ((1u64 << shift) - 1)) == 0;
        fractional_mantissa >>= shift;
    }

    // The mantissa: the integer part's bits, then the fractional part's.
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

    // The exponent: one less than the integer part's bits, or the fractional exponent negated;
    // then one less for the extra bit kept for rounding.
    let final_exponent = if integer_bits_of_precision > 0 {
        integer_bits_of_precision as i32 - 2
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
/// `s[next..]`, with the sign already read; `format` `Hex` reads hexadecimal digits (without `0x`)
/// and a binary exponent after `p`. Under [`Rules::Ucrt`] the digits after the 768th are ignored
/// and the exponent is held to ±5200 before the digits' place is added.
fn ordinary_floating_from_chars<T: FromCharsFloat>(
    s: &[u8],
    value: &mut T,
    format: CharsFormat,
    minus_sign: bool,
    mut next: usize,
    rules: Rules,
) -> FromCharsResult {
    const MAXIMUM_TEMPORARY_DECIMAL_EXPONENT: i64 = 5200;
    const MINIMUM_TEMPORARY_DECIMAL_EXPONENT: i64 = -5200;

    let is_hexadecimal = format == CharsFormat::Hex;
    let base: u8 = if is_hexadecimal { 16 } else { 10 };
    let last = s.len();

    let mut fp_string = FloatingPointString {
        is_negative: minus_sign,
        exponent: 0,
        mantissa_count: 0,
        mantissa: [0; 768],
    };
    let mantissa_last = fp_string.mantissa.len();
    let mut mantissa_it = 0;

    // [whole_begin, whole_end): the digits before the point, if any.
    let whole_begin = next;

    // The leading zeros of the mantissa.
    while next != last && s[next] == b'0' {
        next += 1;
    }
    let leading_zero_end = next;

    let mut has_zero_tail = true;

    // The integer part.
    while next != last {
        let digit_value = digit_from_char(s[next]);
        if digit_value >= base {
            break;
        }
        if mantissa_it != mantissa_last {
            fp_string.mantissa[mantissa_it] = digit_value;
            mantissa_it += 1;
        } else if rules == Rules::Stl {
            has_zero_tail = has_zero_tail && digit_value == 0;
        }
        next += 1;
    }
    let whole_end = next;

    // The digits of the buffer that come before the point; leading zeros are not counted, so it
    // can be negative: 4 for "03333.111", 0 for "00000.111", -2 for "00000.001".
    let mut exponent_adjustment = (whole_end - leading_zero_end) as i64;

    // [whole_end, dot_end): a point, if there is one.
    if next != last && s[next] == b'.' {
        next += 1;
    }
    let dot_end = next;

    // With no nonzero digit yet, the zeros after the point are skipped too, and counted.
    if exponent_adjustment == 0 {
        while next != last && s[next] == b'0' {
            next += 1;
        }
        exponent_adjustment = dot_end as i64 - next as i64;
    }

    // The fractional part.
    while next != last {
        let digit_value = digit_from_char(s[next]);
        if digit_value >= base {
            break;
        }
        if mantissa_it != mantissa_last {
            fp_string.mantissa[mantissa_it] = digit_value;
            mantissa_it += 1;
        } else if rules == Rules::Stl {
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

    // chars_format::fixed takes no exponent part.
    if format != CharsFormat::Fixed && next != last && (s[next] | 0x20) == exponent_prefix {
        let mut unread = next + 1;

        if unread != last && (s[unread] == b'+' || s[unread] == b'-') {
            exponent_is_negative = s[unread] == b'-';
            unread += 1;
        }

        while unread != last {
            let digit_value = digit_from_char(s[unread]);
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
            // The exponent part is read only once a digit of it is.
            next = unread;
        }

        if exponent_is_negative {
            exponent = -exponent;
        }
    }

    // [frac_end, exponent_end): empty, or "[EPep] sign[opt] digit-sequence".
    let exponent_end = next;

    // chars_format::scientific must have the exponent part.
    if format == CharsFormat::Scientific && frac_end == exponent_end {
        return invalid_argument(0);
    }

    // The mantissa's trailing zeros.
    while mantissa_it != 0 && fp_string.mantissa[mantissa_it - 1] == 0 {
        mantissa_it -= 1;
    }

    // No digit left: the mantissa is zero, and so is the value, whatever the exponent. This comes
    // after the exponent is read, so that the end is past it.
    if mantissa_it == 0 {
        debug_assert!(has_zero_tail);
        *value = assemble_zero(fp_string.is_negative);
        return FromCharsResult {
            ptr: next,
            ec: None,
        };
    }

    // An exponent too large for the type.
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

    // Rules::Ucrt: the exponent as written is held to ±5200 before the digits' place is added.
    if rules == Rules::Ucrt {
        if exponent > MAXIMUM_TEMPORARY_DECIMAL_EXPONENT {
            *value = assemble_infinity(fp_string.is_negative);
            return FromCharsResult {
                ptr: next,
                ec: Some(Errc::ResultOutOfRange),
            };
        }
        if exponent < MINIMUM_TEMPORARY_DECIMAL_EXPONENT {
            *value = assemble_zero(fp_string.is_negative);
            return FromCharsResult {
                ptr: next,
                ec: Some(Errc::ResultOutOfRange),
            };
        }
    }

    // An exponent and an adjustment of opposite signs are brought together first, so that neither
    // overflows.
    if exponent > 0 && exponent_adjustment < 0 {
        if is_hexadecimal {
            let further_adjustment = crate::cpp_max(-((exponent - 1) / 4 + 1), exponent_adjustment);
            exponent += further_adjustment * 4;
            exponent_adjustment -= further_adjustment;
        } else {
            let further_adjustment = crate::cpp_max(-exponent, exponent_adjustment);
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

    // A hexadecimal exponent is binary, and its adjustment counts hexadecimal digits: times
    // log2(16).
    let exponent_adjustment_multiplier: i64 = if is_hexadecimal { 4 } else { 1 };

    // Both are now of one sign (or zero), so an out-of-range value shows in either.
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

    // Out of every type's range after the adjustment.
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
        convert_decimal_string(&fp_string, has_zero_tail, rules)
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
        ordinary_floating_from_chars(s, value, format, minus_sign, next, Rules::Stl)
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
        Some(nul) => &s[..nul],
        None => s,
    }
}

/// `isspace` of the "C" locale (C17 7.4.1.10): space, `\t`, `\n`, `\v`, `\f` and `\r`.
const fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// What a `strto*` subject sequence starts with (C17 7.22.1.3 and 7.22.1.4): white space, then an
/// optional `+` or `-`. The index after them, and whether the sign is `-`.
fn space_and_sign(s: &[u8]) -> (usize, bool) {
    let mut i = 0;
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    let negative = s.get(i) == Some(&b'-');
    if negative || s.get(i) == Some(&b'+') {
        i += 1;
    }
    (i, negative)
}

/// What the UCRT's `strtod` and `strtof` make of what follows `nan` at `s[next..]`, as black-box
/// tests of them show (`tests/fmt.rs`, `ucrt_nan_spellings`): an n-char-sequence (letters,
/// digits and `_`) closed by `)` is read with the parentheses, and nothing else is. In any case,
/// `snan` is the signalling NaN, and `ind` after nothing, `s`, `sn`, `sna` or `snan` the
/// indeterminate one; any other sequence is the plain NaN.
fn ucrt_nan_sequence(s: &[u8], next: usize) -> (NanSequence, usize) {
    if s.get(next) != Some(&b'(') {
        return (NanSequence::None, next);
    }
    let sequence_begin = next + 1;
    let mut close = sequence_begin;
    while close < s.len() && (s[close] == b'_' || s[close].is_ascii_alphanumeric()) {
        close += 1;
    }
    if s.get(close) != Some(&b')') {
        return (NanSequence::None, next);
    }

    let sequence = &s[sequence_begin..close];
    let kind = if sequence.len() == 4 && starts_with_case_insensitive(sequence, b"snan") {
        NanSequence::Snan
    } else if let Some(before) = sequence.len().checked_sub(3)
        && before <= 4
        && starts_with_case_insensitive(&sequence[before..], b"ind")
        && starts_with_case_insensitive(&sequence[..before], &b"snan"[..before])
    {
        NanSequence::Ind
    } else {
        NanSequence::Other
    };
    (kind, close + 1)
}

/// The NaN the UCRT's `strtod` and `strtof` give for `nan` followed by `sequence`, `negative` for
/// a `-` before it: plain `nan` is the quiet NaN with every payload bit set (the STL's
/// `_Nan_from_chars` says so too); `nan(snan)` the signalling NaN with payload 1; `nan(ind)` the
/// negative quiet NaN without payload, whatever the sign.
fn ucrt_nan<T: FromCharsFloat>(sequence: NanSequence, negative: bool) -> T {
    let sign = if negative { T::SHIFTED_SIGN_MASK } else { 0 };
    T::from_uint(match sequence {
        NanSequence::None | NanSequence::Other => {
            sign | T::SHIFTED_EXPONENT_MASK | T::DENORMAL_MANTISSA_MASK
        }
        NanSequence::Snan => sign | T::SHIFTED_EXPONENT_MASK | 1,
        NanSequence::Ind => {
            T::SHIFTED_SIGN_MASK | T::SHIFTED_EXPONENT_MASK | T::SPECIAL_NAN_MANTISSA_MASK
        }
    })
}

/// Whether `s[i..]` begins a hexadecimal number after `0x`: a hexadecimal digit, or a point and
/// one (C17 6.4.4.2's hexadecimal-fractional-constant needs a digit on one side of the point).
fn hexadecimal_digits_follow(s: &[u8], i: usize) -> bool {
    match s.get(i) {
        Some(c) if c.is_ascii_hexdigit() => true,
        Some(b'.') => s.get(i + 1).is_some_and(u8::is_ascii_hexdigit),
        _ => false,
    }
}

/// `strtod` and `strtof` (C17 7.22.1.3) in the "C" locale: white space, an optional sign, then the
/// longest subject sequence of a decimal number, `0x` and a hexadecimal one, `inf` or `infinity`,
/// or `nan` or `nan(n-char-sequence)`, in any case. Numbers go through the STL's parser with
/// [`Rules::Ucrt`]; `nan` is read by [`ucrt_nan_sequence`]. `0x` without a hexadecimal digit after
/// it is the subject sequence `0`. A value out of range is the infinity or zero it rounds to, with
/// `ERANGE`; a subnormal result is no range error. With no subject sequence, the value is zero
/// and nothing is read.
fn strto_float<T: FromCharsFloat>(s: &[u8]) -> Strto<T> {
    let no_conversion = Strto {
        value: T::zero(),
        end: 0,
        errno: None,
    };
    let s = c_string(s);
    let (start, negative) = space_and_sign(s);
    let Some(&first) = s.get(start) else {
        return no_conversion;
    };

    let mut value = T::zero();
    let result = match first | 0x20 {
        b'i' => infinity_from_chars(s, &mut value, negative, start),
        b'n' if starts_with_case_insensitive(&s[start + 1..], b"an") => {
            let (sequence, end) = ucrt_nan_sequence(s, start + 3);
            value = ucrt_nan(sequence, negative);
            FromCharsResult { ptr: end, ec: None }
        }
        b'0' if s.get(start + 1).is_some_and(|&c| (c | 0x20) == b'x')
            && hexadecimal_digits_follow(s, start + 2) =>
        {
            ordinary_floating_from_chars(
                s,
                &mut value,
                CharsFormat::Hex,
                negative,
                start + 2,
                Rules::Ucrt,
            )
        }
        _ if first.is_ascii_digit() || first == b'.' => ordinary_floating_from_chars(
            s,
            &mut value,
            CharsFormat::General,
            negative,
            start,
            Rules::Ucrt,
        ),
        _ => invalid_argument(0),
    };

    match result.ec {
        Some(Errc::InvalidArgument) => no_conversion,
        Some(Errc::ResultOutOfRange) => Strto {
            value,
            end: result.ptr,
            errno: Some(Errno::Range),
        },
        Some(Errc::ValueTooLarge) | None => Strto {
            value,
            end: result.ptr,
            errno: None,
        },
    }
}

/// `strtod`.
#[must_use]
pub fn strtod(s: &[u8]) -> Strto<f64> {
    strto_float(s)
}

/// `strtof`: read to `float` directly, not through `double` (no double rounding; `tests/fmt.rs`
/// compares it with the UCRT's).
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
