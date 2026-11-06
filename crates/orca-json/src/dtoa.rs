// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.
//
// Carries JSON for Modern C++ code: this file is translated from
// include/nlohmann/detail/conversions/to_chars.hpp of nlohmann/json 3.10.4, as OrcaSlicer v2.4.2
// carries it in deps_src/nlohmann/detail/conversions/to_chars.hpp (dtoa_impl's diyfp,
// compute_boundaries, get_cached_power_for_binary_exponent, grisu2_digit_gen, grisu2_round,
// grisu2, append_exponent and format_buffer, and to_chars), function for function, on
// 2026-11-06. Its copyright holders are Niels Lohmann (2013-2019) and, for the Grisu2 reference
// implementation it modifies, Florian Loitsch (2009), both under the MIT licence
// (licenses/MIT-nlohmann-json.txt).

//! `nlohmann::detail::to_chars` for a `double`: Grisu2's digits, which read back as the same
//! double but are not always the shortest, printed in fixed notation for decimal exponents from
//! -4 to 15 and in `1.5e+20` notation otherwise, always with a `.0` or an exponent so the text is
//! read as a floating-point number again.

struct Diyfp {
    f: u64,
    e: i32,
}

impl Diyfp {
    const fn new(f: u64, e: i32) -> Diyfp {
        Diyfp { f, e }
    }

    fn sub(x: &Diyfp, y: &Diyfp) -> Diyfp {
        debug_assert!(x.e == y.e && x.f >= y.f);
        Diyfp::new(x.f - y.f, x.e)
    }

    /// The upper 64 bits of the 128-bit product, rounded.
    fn mul(x: &Diyfp, y: &Diyfp) -> Diyfp {
        let u_lo = x.f & 0xFFFF_FFFF;
        let u_hi = x.f >> 32;
        let v_lo = y.f & 0xFFFF_FFFF;
        let v_hi = y.f >> 32;
        let p0 = u_lo * v_lo;
        let p1 = u_lo * v_hi;
        let p2 = u_hi * v_lo;
        let p3 = u_hi * v_hi;
        let p0_hi = p0 >> 32;
        let p1_lo = p1 & 0xFFFF_FFFF;
        let p1_hi = p1 >> 32;
        let p2_lo = p2 & 0xFFFF_FFFF;
        let p2_hi = p2 >> 32;
        let mut q = p0_hi + p1_lo + p2_lo;
        q += 1u64 << (64 - 32 - 1);
        let h = p3 + p2_hi + p1_hi + (q >> 32);
        Diyfp::new(h, x.e + y.e + 64)
    }

    fn normalize(mut x: Diyfp) -> Diyfp {
        debug_assert!(x.f != 0);
        while (x.f >> 63) == 0 {
            x.f <<= 1;
            x.e -= 1;
        }
        x
    }

    fn normalize_to(x: &Diyfp, target_exponent: i32) -> Diyfp {
        let delta = x.e - target_exponent;
        debug_assert!(delta >= 0);
        Diyfp::new(x.f << delta, target_exponent)
    }
}

/// The normalised value and its boundaries `(minus, w, plus)`, for a finite positive double.
fn compute_boundaries(value: f64) -> (Diyfp, Diyfp, Diyfp) {
    const PRECISION: i32 = 53;
    const BIAS: i32 = 1024 - 1 + (PRECISION - 1);
    const MIN_EXP: i32 = 1 - BIAS;
    const HIDDEN_BIT: u64 = 1u64 << (PRECISION - 1);
    let bits = value.to_bits();
    let e = bits >> (PRECISION - 1);
    let f = bits & (HIDDEN_BIT - 1);
    let is_denormal = e == 0;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let v = if is_denormal {
        Diyfp::new(f, MIN_EXP)
    } else {
        Diyfp::new(f + HIDDEN_BIT, e as i32 - BIAS)
    };
    let lower_boundary_is_closer = f == 0 && e > 1;
    let m_plus = Diyfp::new(2 * v.f + 1, v.e - 1);
    let m_minus = if lower_boundary_is_closer {
        Diyfp::new(4 * v.f - 1, v.e - 2)
    } else {
        Diyfp::new(2 * v.f - 1, v.e - 1)
    };
    let w_plus = Diyfp::normalize(m_plus);
    let w_minus = Diyfp::normalize_to(&m_minus, w_plus.e);
    (w_minus, Diyfp::normalize(v), w_plus)
}

const ALPHA: i32 = -60;

/// `kCachedPowers`: (f, e, k), 10^k ~ f * 2^e, for k = -300 to 324 in steps of 8.
const CACHED_POWERS: [(u64, i32, i32); 79] = [
    (0xAB70_FE17_C79A_C6CA, -1060, -300),
    (0xFF77_B1FC_BEBC_DC4F, -1034, -292),
    (0xBE56_91EF_416B_D60C, -1007, -284),
    (0x8DD0_1FAD_907F_FC3C, -980, -276),
    (0xD351_5C28_3155_9A83, -954, -268),
    (0x9D71_AC8F_ADA6_C9B5, -927, -260),
    (0xEA9C_2277_23EE_8BCB, -901, -252),
    (0xAECC_4991_4078_536D, -874, -244),
    (0x823C_1279_5DB6_CE57, -847, -236),
    (0xC210_9436_4DFB_5637, -821, -228),
    (0x9096_EA6F_3848_984F, -794, -220),
    (0xD774_85CB_2582_3AC7, -768, -212),
    (0xA086_CFCD_97BF_97F4, -741, -204),
    (0xEF34_0A98_172A_ACE5, -715, -196),
    (0xB238_67FB_2A35_B28E, -688, -188),
    (0x84C8_D4DF_D2C6_3F3B, -661, -180),
    (0xC5DD_4427_1AD3_CDBA, -635, -172),
    (0x936B_9FCE_BB25_C996, -608, -164),
    (0xDBAC_6C24_7D62_A584, -582, -156),
    (0xA3AB_6658_0D5F_DAF6, -555, -148),
    (0xF3E2_F893_DEC3_F126, -529, -140),
    (0xB5B5_ADA8_AAFF_80B8, -502, -132),
    (0x8762_5F05_6C7C_4A8B, -475, -124),
    (0xC9BC_FF60_34C1_3053, -449, -116),
    (0x964E_858C_91BA_2655, -422, -108),
    (0xDFF9_7724_7029_7EBD, -396, -100),
    (0xA6DF_BD9F_B8E5_B88F, -369, -92),
    (0xF8A9_5FCF_8874_7D94, -343, -84),
    (0xB944_7093_8FA8_9BCF, -316, -76),
    (0x8A08_F0F8_BF0F_156B, -289, -68),
    (0xCDB0_2555_6531_31B6, -263, -60),
    (0x993F_E2C6_D07B_7FAC, -236, -52),
    (0xE45C_10C4_2A2B_3B06, -210, -44),
    (0xAA24_2499_6973_92D3, -183, -36),
    (0xFD87_B5F2_8300_CA0E, -157, -28),
    (0xBCE5_0864_9211_1AEB, -130, -20),
    (0x8CBC_CC09_6F50_88CC, -103, -12),
    (0xD1B7_1758_E219_652C, -77, -4),
    (0x9C40_0000_0000_0000, -50, 4),
    (0xE8D4_A510_0000_0000, -24, 12),
    (0xAD78_EBC5_AC62_0000, 3, 20),
    (0x813F_3978_F894_0984, 30, 28),
    (0xC097_CE7B_C907_15B3, 56, 36),
    (0x8F7E_32CE_7BEA_5C70, 83, 44),
    (0xD5D2_38A4_ABE9_8068, 109, 52),
    (0x9F4F_2726_179A_2245, 136, 60),
    (0xED63_A231_D4C4_FB27, 162, 68),
    (0xB0DE_6538_8CC8_ADA8, 189, 76),
    (0x83C7_088E_1AAB_65DB, 216, 84),
    (0xC45D_1DF9_4271_1D9A, 242, 92),
    (0x924D_692C_A61B_E758, 269, 100),
    (0xDA01_EE64_1A70_8DEA, 295, 108),
    (0xA26D_A399_9AEF_774A, 322, 116),
    (0xF209_787B_B47D_6B85, 348, 124),
    (0xB454_E4A1_79DD_1877, 375, 132),
    (0x865B_8692_5B9B_C5C2, 402, 140),
    (0xC835_53C5_C896_5D3D, 428, 148),
    (0x952A_B45C_FA97_A0B3, 455, 156),
    (0xDE46_9FBD_99A0_5FE3, 481, 164),
    (0xA59B_C234_DB39_8C25, 508, 172),
    (0xF6C6_9A72_A398_9F5C, 534, 180),
    (0xB7DC_BF53_54E9_BECE, 561, 188),
    (0x88FC_F317_F222_41E2, 588, 196),
    (0xCC20_CE9B_D35C_78A5, 614, 204),
    (0x9816_5AF3_7B21_53DF, 641, 212),
    (0xE2A0_B5DC_971F_303A, 667, 220),
    (0xA8D9_D153_5CE3_B396, 694, 228),
    (0xFB9B_7CD9_A4A7_443C, 720, 236),
    (0xBB76_4C4C_A7A4_4410, 747, 244),
    (0x8BAB_8EEF_B640_9C1A, 774, 252),
    (0xD01F_EF10_A657_842C, 800, 260),
    (0x9B10_A4E5_E991_3129, 827, 268),
    (0xE710_9BFB_A19C_0C9D, 853, 276),
    (0xAC28_20D9_623B_F429, 880, 284),
    (0x8044_4B5E_7AA7_CF85, 907, 292),
    (0xBF21_E440_03AC_DD2D, 933, 300),
    (0x8E67_9C2F_5E44_FF8F, 960, 308),
    (0xD433_179D_9C8C_B841, 986, 316),
    (0x9E19_DB92_B4E3_1BA9, 1013, 324),
];

/// The cached power c = f_c * 2^e_c such that -60 <= e_c + e + 64 <= -32.
fn cached_power_for_binary_exponent(e: i32) -> (u64, i32, i32) {
    const MIN_DEC_EXP: i32 = -300;
    const DEC_STEP: i32 = 8;
    let f = ALPHA - e - 1;
    let k = (f * 78913) / (1 << 18) + i32::from(f > 0);
    let index = (-MIN_DEC_EXP + k + (DEC_STEP - 1)) / DEC_STEP;
    let cached = CACHED_POWERS[usize::try_from(index).expect("a cached power's index")];
    debug_assert!(ALPHA <= cached.1 + e + 64 && cached.1 + e + 64 <= -32);
    cached
}

/// (k, 10^(k-1)) such that 10^(k-1) <= n < 10^k; (1, 1) for n = 0.
fn find_largest_pow10(n: u32) -> (i32, u32) {
    let mut pow10 = 1_000_000_000u32;
    let mut k = 10;
    while k > 1 && n < pow10 {
        pow10 /= 10;
        k -= 1;
    }
    (k, pow10)
}

fn grisu2_round(buf: &mut [u8], len: usize, dist: u64, delta: u64, mut rest: u64, ten_k: u64) {
    while rest < dist
        && delta - rest >= ten_k
        && (rest + ten_k < dist || dist - rest > rest + ten_k - dist)
    {
        buf[len - 1] -= 1;
        rest += ten_k;
    }
}

/// Generates the digits of V = buffer * 10^decimal_exponent with M- <= V <= M+.
fn grisu2_digit_gen(
    buffer: &mut [u8],
    length: &mut usize,
    decimal_exponent: &mut i32,
    m_minus: &Diyfp,
    w: &Diyfp,
    m_plus: &Diyfp,
) {
    let mut delta = Diyfp::sub(m_plus, m_minus).f;
    let mut dist = Diyfp::sub(m_plus, w).f;
    let one = Diyfp::new(1u64 << -m_plus.e, m_plus.e);
    #[allow(clippy::cast_possible_truncation)]
    let mut p1 = (m_plus.f >> -one.e) as u32;
    let mut p2 = m_plus.f & (one.f - 1);
    debug_assert!(p1 > 0);
    let (k, mut pow10) = find_largest_pow10(p1);
    let mut n = k;
    while n > 0 {
        let d = p1 / pow10;
        let r = p1 % pow10;
        #[allow(clippy::cast_possible_truncation)]
        {
            buffer[*length] = b'0' + d as u8;
        }
        *length += 1;
        p1 = r;
        n -= 1;
        let rest = (u64::from(p1) << -one.e) + p2;
        if rest <= delta {
            *decimal_exponent += n;
            let ten_n = u64::from(pow10) << -one.e;
            grisu2_round(buffer, *length, dist, delta, rest, ten_n);
            return;
        }
        pow10 /= 10;
    }
    let mut m = 0;
    loop {
        p2 *= 10;
        let d = p2 >> -one.e;
        let r = p2 & (one.f - 1);
        #[allow(clippy::cast_possible_truncation)]
        {
            buffer[*length] = b'0' + d as u8;
        }
        *length += 1;
        p2 = r;
        m += 1;
        delta *= 10;
        dist *= 10;
        if p2 <= delta {
            break;
        }
    }
    *decimal_exponent -= m;
    grisu2_round(buffer, *length, dist, delta, p2, one.f);
}

/// Grisu2's digits of a finite positive double and their decimal exponent.
fn grisu2(value: f64) -> (Vec<u8>, i32) {
    let (m_minus, v, m_plus) = compute_boundaries(value);
    let (cf, ce, ck) = cached_power_for_binary_exponent(m_plus.e);
    let c_minus_k = Diyfp::new(cf, ce);
    let w = Diyfp::mul(&v, &c_minus_k);
    let w_minus = Diyfp::mul(&m_minus, &c_minus_k);
    let w_plus = Diyfp::mul(&m_plus, &c_minus_k);
    let lower = Diyfp::new(w_minus.f + 1, w_minus.e);
    let upper = Diyfp::new(w_plus.f - 1, w_plus.e);
    let mut buf = [0u8; 32];
    let mut len = 0usize;
    let mut decimal_exponent = -ck;
    grisu2_digit_gen(
        &mut buf,
        &mut len,
        &mut decimal_exponent,
        &lower,
        &w,
        &upper,
    );
    (buf[..len].to_vec(), decimal_exponent)
}

fn append_exponent(out: &mut Vec<u8>, mut e: i32) {
    if e < 0 {
        e = -e;
        out.push(b'-');
    } else {
        out.push(b'+');
    }
    let k = e.unsigned_abs();
    #[allow(clippy::cast_possible_truncation)]
    let digit = |x: u32| b'0' + x as u8;
    if k < 10 {
        out.push(b'0');
        out.push(digit(k));
    } else if k < 100 {
        out.push(digit(k / 10));
        out.push(digit(k % 10));
    } else {
        out.push(digit(k / 100));
        out.push(digit(k % 100 / 10));
        out.push(digit(k % 10));
    }
}

/// `format_buffer(buf, len, decimal_exponent, -4, 15)`.
fn format_buffer(out: &mut Vec<u8>, digits: &[u8], decimal_exponent: i32) {
    const MIN_EXP: i32 = -4;
    const MAX_EXP: i32 = 15;
    let k = i32::try_from(digits.len()).expect("at most 17 digits");
    let n = k + decimal_exponent;
    let count = |x: i32| usize::try_from(x).expect("a count");
    if k <= n && n <= MAX_EXP {
        out.extend_from_slice(digits);
        out.resize(out.len() + count(n - k), b'0');
        out.extend_from_slice(b".0");
        return;
    }
    if 0 < n && n <= MAX_EXP {
        out.extend_from_slice(&digits[..count(n)]);
        out.push(b'.');
        out.extend_from_slice(&digits[count(n)..]);
        return;
    }
    if MIN_EXP < n && n <= 0 {
        out.extend_from_slice(b"0.");
        out.resize(out.len() + count(-n), b'0');
        out.extend_from_slice(digits);
        return;
    }
    out.push(digits[0]);
    if k > 1 {
        out.push(b'.');
        out.extend_from_slice(&digits[1..]);
    }
    out.push(b'e');
    append_exponent(out, n - 1);
}

/// `nlohmann::detail::to_chars(first, last, value)` for a finite double, appended to `out`.
///
/// # Panics
///
/// For an infinity or a NaN, which the serializer writes as `null` before it gets here.
pub fn to_chars(out: &mut Vec<u8>, mut value: f64) {
    assert!(value.is_finite(), "to_chars takes a finite value");
    if value.is_sign_negative() {
        value = -value;
        out.push(b'-');
    }
    if value == 0.0 {
        out.extend_from_slice(b"0.0");
        return;
    }
    let (digits, decimal_exponent) = grisu2(value);
    format_buffer(out, &digits, decimal_exponent);
}
