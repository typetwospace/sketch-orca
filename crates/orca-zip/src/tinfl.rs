// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries miniz code: tinfl_decompress, its macros (TINFL_CR_BEGIN, TINFL_CR_RETURN,
// TINFL_GET_BYTE, TINFL_NEED_BITS, TINFL_SKIP_BITS, TINFL_GET_BITS, TINFL_HUFF_BITBUF_FILL,
// TINFL_HUFF_DECODE) and tinfl_decompressor of deps_src/miniz/miniz.c and miniz.h (miniz 2.1.0,
// MZ_VERSION "10.1.0") as OrcaSlicer v2.4.2 carries it, translated on 2026-11-07. Copyright
// 2013-2014 RAD Game Tools and Valve Software and 2010-2014 Rich Geldreich and Tenacious Software
// LLC (the holders), under the MIT licence (licenses/MIT-miniz.txt).

//! `tinfl_decompress`: miniz's inflater, a coroutine that stops wherever its input runs out or its
//! output is full and goes on from there on the next call.
//!
//! The C function is one `switch` on `m_state` whose cases sit inside its loops
//! (`TINFL_CR_RETURN`). Here [`Decompressor::decompress`] keeps the C's state numbers and resumes
//! at the same points: `TINFL_GET_BYTE`, `TINFL_NEED_BITS` and `TINFL_GET_BITS` start again from
//! their top, which is the same as resuming inside them (the condition that made them stop still
//! holds), and `TINFL_HUFF_DECODE` resumes inside `TINFL_HUFF_BITBUF_FILL`, where its only stop
//! is, reading one byte at a time. Where the input runs out decides how much output a call
//! returns, and so how the zip reader's callback ([`crate::reader`]) splits an entry: the 64-bit
//! bit buffer (`TINFL_USE_64BIT_BITBUF`, as `MINIZ_HAS_64BIT_REGISTERS` is set on x64), the
//! four-byte read-ahead of the fast literal loop and the bytes put back at the end are kept as
//! the C has them. The C's eight-byte copies of a match (`MINIZ_USE_UNALIGNED_LOADS_AND_STORES`
//! with `MINIZ_UNALIGNED_USE_MEMCPY`: one 8-byte load, then one store) are kept: in a ring smaller
//! than a match's distance the source can sit just behind the output, and a byte copy would then
//! write other bytes (the zip reader's ring is the whole window, so it never does).

/// `TINFL_FLAG_PARSE_ZLIB_HEADER`.
pub const TINFL_FLAG_PARSE_ZLIB_HEADER: u32 = 1;
/// `TINFL_FLAG_HAS_MORE_INPUT`.
pub const TINFL_FLAG_HAS_MORE_INPUT: u32 = 2;
/// `TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF`.
pub const TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF: u32 = 4;
/// `TINFL_FLAG_COMPUTE_ADLER32`.
pub const TINFL_FLAG_COMPUTE_ADLER32: u32 = 8;

/// `TINFL_LZ_DICT_SIZE`.
pub const TINFL_LZ_DICT_SIZE: usize = 32768;

const TINFL_FAST_LOOKUP_BITS: u32 = 10;
const TINFL_FAST_LOOKUP_SIZE: usize = 1 << TINFL_FAST_LOOKUP_BITS;
const TINFL_MAX_HUFF_SYMBOLS_0: usize = 288;

/// `tinfl_status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// `TINFL_STATUS_FAILED_CANNOT_MAKE_PROGRESS` (-4).
    FailedCannotMakeProgress,
    /// `TINFL_STATUS_BAD_PARAM` (-3).
    BadParam,
    /// `TINFL_STATUS_ADLER32_MISMATCH` (-2).
    Adler32Mismatch,
    /// `TINFL_STATUS_FAILED` (-1).
    Failed,
    /// `TINFL_STATUS_DONE` (0).
    Done,
    /// `TINFL_STATUS_NEEDS_MORE_INPUT` (1).
    NeedsMoreInput,
    /// `TINFL_STATUS_HAS_MORE_OUTPUT` (2).
    HasMoreOutput,
}

impl Status {
    /// The C enumerator's value.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Status::FailedCannotMakeProgress => -4,
            Status::BadParam => -3,
            Status::Adler32Mismatch => -2,
            Status::Failed => -1,
            Status::Done => 0,
            Status::NeedsMoreInput => 1,
            Status::HasMoreOutput => 2,
        }
    }
}

/// `tinfl_huff_table`.
#[derive(Clone)]
struct HuffTable {
    code_size: [u8; TINFL_MAX_HUFF_SYMBOLS_0],
    look_up: [i16; TINFL_FAST_LOOKUP_SIZE],
    tree: [i16; TINFL_MAX_HUFF_SYMBOLS_0 * 2],
}

impl HuffTable {
    fn new() -> Self {
        HuffTable {
            code_size: [0; TINFL_MAX_HUFF_SYMBOLS_0],
            look_up: [0; TINFL_FAST_LOOKUP_SIZE],
            tree: [0; TINFL_MAX_HUFF_SYMBOLS_0 * 2],
        }
    }
}

/// `tinfl_decompressor`. `Decompressor::new()` is a fresh one after `tinfl_init` (the C leaves the
/// other fields as the stack had them; every one is set before it is read).
#[derive(Clone)]
pub struct Decompressor {
    state: u32,
    num_bits: u32,
    zhdr0: u32,
    zhdr1: u32,
    z_adler32: u32,
    final_: u32,
    type_: u32,
    check_adler32: u32,
    dist: u32,
    counter: u32,
    num_extra: u32,
    table_sizes: [u32; 3],
    bit_buf: u64,
    dist_from_out_buf_start: usize,
    tables: [HuffTable; 3],
    raw_header: [u8; 4],
    len_codes: [u8; TINFL_MAX_HUFF_SYMBOLS_0 + 32 + 137],
}

impl Default for Decompressor {
    fn default() -> Self {
        Self::new()
    }
}

/// What one call reports: the status and how many input bytes it took and output bytes it wrote
/// (`*pIn_buf_size` and `*pOut_buf_size` on return).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    /// The status.
    pub status: Status,
    /// The input bytes consumed.
    pub in_consumed: usize,
    /// The output bytes written.
    pub out_written: usize,
}

const S_LENGTH_BASE: [u32; 31] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258, 0, 0,
];
const S_LENGTH_EXTRA: [u32; 31] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0, 0, 0,
];
const S_DIST_BASE: [u32; 32] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577, 0, 0,
];
const S_DIST_EXTRA: [u32; 32] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13, 0, 0,
];
const S_LENGTH_DEZIGZAG: [u8; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];
const S_MIN_TABLE_SIZES: [u32; 3] = [257, 1, 4];

/// The live locals of one call, which the C keeps in registers between its stops.
struct Ctx<'a> {
    input: &'a [u8],
    in_cur: usize,
    out: &'a mut [u8],
    out_cur: usize,
    out_end: usize,
    num_bits: u32,
    bit_buf: u64,
    flags: u32,
}

impl Ctx<'_> {
    /// `TINFL_GET_BYTE`: `None` where the C stops (the input is used up).
    fn get_byte(&mut self) -> Option<u32> {
        if self.in_cur >= self.input.len() {
            return None;
        }
        let c = u32::from(self.input[self.in_cur]);
        self.in_cur += 1;
        Some(c)
    }

    /// `TINFL_NEED_BITS(n)` after its `if (num_bits < n)`.
    fn need_bits(&mut self, n: u32) -> bool {
        loop {
            let Some(c) = self.get_byte() else {
                return false;
            };
            self.bit_buf |= u64::from(c) << self.num_bits;
            self.num_bits += 8;
            if self.num_bits >= n {
                return true;
            }
        }
    }

    /// `TINFL_GET_BITS(n)`.
    fn get_bits(&mut self, n: u32) -> Option<u32> {
        if self.num_bits < n && !self.need_bits(n) {
            return None;
        }
        let b = (self.bit_buf & ((1u64 << n) - 1)) as u32;
        self.bit_buf >>= n;
        self.num_bits -= n;
        Some(b)
    }

    /// `TINFL_SKIP_BITS(n)`.
    fn skip_bits(&mut self, n: u32) -> bool {
        if self.num_bits < n && !self.need_bits(n) {
            return false;
        }
        self.bit_buf >>= n;
        self.num_bits -= n;
        true
    }

    /// `TINFL_HUFF_DECODE`: the next symbol of `t`, or `None` where it stops (in
    /// `TINFL_HUFF_BITBUF_FILL`, the input used up). `resume` goes on from that stop.
    fn huff_decode(&mut self, t: &HuffTable, resume: bool) -> Option<i32> {
        if resume || self.num_bits < 15 {
            if resume || self.input.len() - self.in_cur < 2 {
                // TINFL_HUFF_BITBUF_FILL.
                let mut skip_try = resume;
                loop {
                    if !skip_try {
                        let mut temp = i32::from(
                            t.look_up
                                [(self.bit_buf & (TINFL_FAST_LOOKUP_SIZE as u64 - 1)) as usize],
                        );
                        if temp >= 0 {
                            let code_len = (temp >> 9) as u32;
                            if code_len != 0 && self.num_bits >= code_len {
                                break;
                            }
                        } else if self.num_bits > TINFL_FAST_LOOKUP_BITS {
                            let mut code_len = TINFL_FAST_LOOKUP_BITS;
                            loop {
                                temp = i32::from(
                                    t.tree[(!temp + ((self.bit_buf >> code_len) & 1) as i32)
                                        as usize],
                                );
                                code_len += 1;
                                if !(temp < 0 && self.num_bits > code_len) {
                                    break;
                                }
                            }
                            if temp >= 0 {
                                break;
                            }
                        }
                    }
                    skip_try = false;
                    let c = self.get_byte()?;
                    self.bit_buf |= u64::from(c) << self.num_bits;
                    self.num_bits += 8;
                    if self.num_bits >= 15 {
                        break;
                    }
                }
            } else {
                self.bit_buf |= (u64::from(self.input[self.in_cur]) << self.num_bits)
                    | (u64::from(self.input[self.in_cur + 1]) << (self.num_bits + 8));
                self.in_cur += 2;
                self.num_bits += 16;
            }
        }
        let mut temp =
            i32::from(t.look_up[(self.bit_buf & (TINFL_FAST_LOOKUP_SIZE as u64 - 1)) as usize]);
        let code_len;
        if temp >= 0 {
            code_len = (temp >> 9) as u32;
            temp &= 511;
        } else {
            let mut cl = TINFL_FAST_LOOKUP_BITS;
            loop {
                temp = i32::from(t.tree[(!temp + ((self.bit_buf >> cl) & 1) as i32) as usize]);
                cl += 1;
                if temp >= 0 {
                    break;
                }
            }
            code_len = cl;
        }
        self.bit_buf >>= code_len;
        self.num_bits -= code_len;
        Some(temp)
    }
}

impl Decompressor {
    /// A decompressor after `tinfl_init`.
    #[must_use]
    pub fn new() -> Self {
        Decompressor {
            state: 0,
            num_bits: 0,
            zhdr0: 0,
            zhdr1: 0,
            z_adler32: 0,
            final_: 0,
            type_: 0,
            check_adler32: 0,
            dist: 0,
            counter: 0,
            num_extra: 0,
            table_sizes: [0; 3],
            bit_buf: 0,
            dist_from_out_buf_start: 0,
            tables: [HuffTable::new(), HuffTable::new(), HuffTable::new()],
            raw_header: [0; 4],
            len_codes: [0; TINFL_MAX_HUFF_SYMBOLS_0 + 32 + 137],
        }
    }

    /// `tinfl_get_adler32`.
    #[must_use]
    pub fn adler32(&self) -> u32 {
        self.check_adler32
    }

    /// `tinfl_decompress(r, input, &in_size, out (pOut_buf_start), out + out_next, &out_size,
    /// flags)`: `out_next + out_size` must lie within `out`.
    pub fn decompress(
        &mut self,
        input: &[u8],
        out: &mut [u8],
        out_next: usize,
        out_size: usize,
        flags: u32,
    ) -> Progress {
        let out_buf_size_mask = if flags & TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF != 0 {
            usize::MAX
        } else {
            out_next + out_size - 1
        };
        if (out_buf_size_mask.wrapping_add(1) & out_buf_size_mask) != 0 {
            return Progress {
                status: Status::BadParam,
                in_consumed: 0,
                out_written: 0,
            };
        }
        let mut c = Ctx {
            input,
            in_cur: 0,
            out,
            out_cur: out_next,
            out_end: out_next + out_size,
            num_bits: self.num_bits,
            bit_buf: self.bit_buf,
            flags,
        };
        let mut dist = self.dist;
        let mut counter = self.counter;
        let mut num_extra = self.num_extra;
        let mut dist_from_out_buf_start = self.dist_from_out_buf_start;
        let status = self.run(
            &mut c,
            &mut dist,
            &mut counter,
            &mut num_extra,
            &mut dist_from_out_buf_start,
            out_buf_size_mask,
        );
        // common_exit.
        if status != Status::NeedsMoreInput && status != Status::FailedCannotMakeProgress {
            while c.in_cur > 0 && c.num_bits >= 8 {
                c.in_cur -= 1;
                c.num_bits -= 8;
            }
        }
        self.num_bits = c.num_bits;
        self.bit_buf = c.bit_buf & ((1u64 << c.num_bits).wrapping_sub(1));
        self.dist = dist;
        self.counter = counter;
        self.num_extra = num_extra;
        self.dist_from_out_buf_start = dist_from_out_buf_start;
        let in_consumed = c.in_cur;
        let out_written = c.out_cur - out_next;
        let mut status = status;
        if (flags & (TINFL_FLAG_PARSE_ZLIB_HEADER | TINFL_FLAG_COMPUTE_ADLER32)) != 0
            && status.code() >= 0
        {
            let mut ptr = &c.out[out_next..out_next + out_written];
            let mut buf_len = out_written;
            let mut s1 = self.check_adler32 & 0xffff;
            let mut s2 = self.check_adler32 >> 16;
            let mut block_len = buf_len % 5552;
            while buf_len != 0 {
                for &b in &ptr[..block_len] {
                    s1 = s1.wrapping_add(u32::from(b));
                    s2 = s2.wrapping_add(s1);
                }
                ptr = &ptr[block_len..];
                s1 %= 65521;
                s2 %= 65521;
                buf_len -= block_len;
                block_len = 5552;
            }
            self.check_adler32 = (s2 << 16).wrapping_add(s1);
            if status == Status::Done
                && (flags & TINFL_FLAG_PARSE_ZLIB_HEADER) != 0
                && self.check_adler32 != self.z_adler32
            {
                status = Status::Adler32Mismatch;
            }
        }
        Progress {
            status,
            in_consumed,
            out_written,
        }
    }

    /// The status a stop at `state` returns: the C's `TINFL_CR_RETURN(state, status)`.
    fn stop(&mut self, state: u32, status: Status) -> Status {
        self.state = state;
        status
    }

    /// What `TINFL_GET_BYTE` returns when the input is used up.
    fn starved(flags: u32) -> Status {
        if flags & TINFL_FLAG_HAS_MORE_INPUT != 0 {
            Status::NeedsMoreInput
        } else {
            Status::FailedCannotMakeProgress
        }
    }

    /// The body of `tinfl_decompress` between `TINFL_CR_BEGIN` and `common_exit`.
    #[allow(clippy::too_many_lines, clippy::too_many_arguments)]
    fn run(
        &mut self,
        c: &mut Ctx<'_>,
        dist: &mut u32,
        counter: &mut u32,
        num_extra: &mut u32,
        dist_from_out_buf_start: &mut usize,
        out_buf_size_mask: usize,
    ) -> Status {
        // The labels the C's cases sit at; `L::Block` is the top of the do-while of blocks.
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum L {
            Begin,
            ZHdr0,
            ZHdr1,
            Block,
            StoredSkip,
            StoredHeader,
            StoredBits,
            StoredBitsOut,
            StoredCopy,
            Dynamic,
            DynamicSizes,
            DynamicCodeLengths,
            BuildTables,
            LenCodes { resume: bool },
            LenCodesExtra,
            LitLoop,
            LitSlow { resume: bool },
            LitSlowOut,
            Length,
            LengthExtra,
            Dist { resume: bool },
            DistExtra,
            CopySlow,
            CopySlowOut,
            EndBlock,
            Tail,
            Adler,
            Forever(u32, Status),
        }
        let flags = c.flags;
        let mut l = match self.state {
            0 => L::Begin,
            1 => L::ZHdr0,
            2 => L::ZHdr1,
            3 => L::Block,
            5 => L::StoredSkip,
            6 | 7 => L::StoredHeader,
            51 => L::StoredBits,
            52 => L::StoredBitsOut,
            9 | 38 => L::StoredCopy,
            11 => L::DynamicSizes,
            14 => L::DynamicCodeLengths,
            16 => L::LenCodes { resume: true },
            18 => L::LenCodesExtra,
            23 => L::LitSlow { resume: true },
            24 => L::LitSlowOut,
            25 => L::LengthExtra,
            26 => L::Dist { resume: true },
            27 => L::DistExtra,
            53 => L::CopySlowOut,
            32 => L::Tail,
            41 | 42 => L::Adler,
            34 => L::Forever(34, Status::Done),
            s @ (10 | 17 | 21 | 35 | 36 | 37 | 39) => L::Forever(s, Status::Failed),
            _ => return Status::Failed,
        };
        loop {
            match l {
                L::Begin => {
                    c.bit_buf = 0;
                    c.num_bits = 0;
                    *dist = 0;
                    *counter = 0;
                    *num_extra = 0;
                    self.zhdr0 = 0;
                    self.zhdr1 = 0;
                    self.z_adler32 = 1;
                    self.check_adler32 = 1;
                    l = if flags & TINFL_FLAG_PARSE_ZLIB_HEADER != 0 {
                        L::ZHdr0
                    } else {
                        L::Block
                    };
                }
                L::ZHdr0 => {
                    let Some(b) = c.get_byte() else {
                        return self.stop(1, Self::starved(flags));
                    };
                    self.zhdr0 = b;
                    l = L::ZHdr1;
                }
                L::ZHdr1 => {
                    let Some(b) = c.get_byte() else {
                        return self.stop(2, Self::starved(flags));
                    };
                    self.zhdr1 = b;
                    let mut bad = u32::from(
                        !(self.zhdr0 * 256 + self.zhdr1).is_multiple_of(31)
                            || (self.zhdr1 & 32) != 0
                            || (self.zhdr0 & 15) != 8,
                    );
                    if flags & TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF == 0 {
                        let window = 1u32.wrapping_shl(8 + (self.zhdr0 >> 4));
                        bad |= u32::from(
                            window > 32768 || out_buf_size_mask.wrapping_add(1) < window as usize,
                        );
                    }
                    *counter = bad;
                    l = if bad != 0 {
                        L::Forever(36, Status::Failed)
                    } else {
                        L::Block
                    };
                }
                L::Block => {
                    let Some(b) = c.get_bits(3) else {
                        return self.stop(3, Self::starved(flags));
                    };
                    self.final_ = b;
                    self.type_ = self.final_ >> 1;
                    l = match self.type_ {
                        0 => L::StoredSkip,
                        3 => L::Forever(10, Status::Failed),
                        _ => L::Dynamic,
                    };
                }
                L::StoredSkip => {
                    let n = c.num_bits & 7;
                    if !c.skip_bits(n) {
                        return self.stop(5, Self::starved(flags));
                    }
                    *counter = 0;
                    l = L::StoredHeader;
                }
                L::StoredHeader => {
                    while *counter < 4 {
                        if c.num_bits != 0 {
                            let Some(b) = c.get_bits(8) else {
                                return self.stop(6, Self::starved(flags));
                            };
                            self.raw_header[*counter as usize] = b as u8;
                        } else {
                            let Some(b) = c.get_byte() else {
                                return self.stop(7, Self::starved(flags));
                            };
                            self.raw_header[*counter as usize] = b as u8;
                        }
                        *counter += 1;
                    }
                    *counter = u32::from(self.raw_header[0]) | (u32::from(self.raw_header[1]) << 8);
                    let check = 0xFFFF
                        ^ (u32::from(self.raw_header[2]) | (u32::from(self.raw_header[3]) << 8));
                    l = if *counter != check {
                        L::Forever(39, Status::Failed)
                    } else {
                        L::StoredBits
                    };
                }
                L::StoredBits => {
                    if *counter != 0 && c.num_bits != 0 {
                        let Some(b) = c.get_bits(8) else {
                            return self.stop(51, Self::starved(flags));
                        };
                        *dist = b;
                        l = L::StoredBitsOut;
                    } else {
                        l = L::StoredCopy;
                    }
                }
                L::StoredBitsOut => {
                    if c.out_cur >= c.out_end {
                        return self.stop(52, Status::HasMoreOutput);
                    }
                    c.out[c.out_cur] = *dist as u8;
                    c.out_cur += 1;
                    *counter -= 1;
                    l = L::StoredBits;
                }
                L::StoredCopy => {
                    if *counter == 0 {
                        l = L::EndBlock;
                        continue;
                    }
                    if c.out_cur >= c.out_end {
                        return self.stop(9, Status::HasMoreOutput);
                    }
                    if c.in_cur >= c.input.len() {
                        return self.stop(38, Self::starved(flags));
                    }
                    let n = (c.out_end - c.out_cur)
                        .min(c.input.len() - c.in_cur)
                        .min(*counter as usize);
                    c.out[c.out_cur..c.out_cur + n]
                        .copy_from_slice(&c.input[c.in_cur..c.in_cur + n]);
                    c.in_cur += n;
                    c.out_cur += n;
                    *counter -= n as u32;
                }
                L::Dynamic => {
                    if self.type_ == 1 {
                        self.table_sizes[0] = 288;
                        self.table_sizes[1] = 32;
                        self.tables[1].code_size[..32].fill(5);
                        let p = &mut self.tables[0].code_size;
                        p[..=143].fill(8);
                        p[144..=255].fill(9);
                        p[256..=279].fill(7);
                        p[280..=287].fill(8);
                        l = L::BuildTables;
                    } else {
                        *counter = 0;
                        l = L::DynamicSizes;
                    }
                }
                L::DynamicSizes => {
                    while *counter < 3 {
                        let n = [5u32, 5, 4][*counter as usize];
                        let Some(b) = c.get_bits(n) else {
                            return self.stop(11, Self::starved(flags));
                        };
                        self.table_sizes[*counter as usize] =
                            b + S_MIN_TABLE_SIZES[*counter as usize];
                        *counter += 1;
                    }
                    self.tables[2].code_size = [0; TINFL_MAX_HUFF_SYMBOLS_0];
                    *counter = 0;
                    l = L::DynamicCodeLengths;
                }
                L::DynamicCodeLengths => {
                    while *counter < self.table_sizes[2] {
                        let Some(s) = c.get_bits(3) else {
                            return self.stop(14, Self::starved(flags));
                        };
                        self.tables[2].code_size[S_LENGTH_DEZIGZAG[*counter as usize] as usize] =
                            s as u8;
                        *counter += 1;
                    }
                    self.table_sizes[2] = 19;
                    l = L::BuildTables;
                }
                L::BuildTables => {
                    // for (; (int)r->m_type >= 0; r->m_type--): one table, then the code lengths
                    // when it is table 2.
                    if (self.type_ as i32) < 0 {
                        l = L::LitLoop;
                        continue;
                    }
                    if !self.build_table(self.type_ as usize) {
                        l = L::Forever(35, Status::Failed);
                        continue;
                    }
                    if self.type_ == 2 {
                        *counter = 0;
                        l = L::LenCodes { resume: false };
                    } else {
                        self.type_ = self.type_.wrapping_sub(1);
                    }
                }
                L::LenCodes { resume } => {
                    let mut resume = resume;
                    loop {
                        if *counter >= self.table_sizes[0] + self.table_sizes[1] {
                            break;
                        }
                        let Some(sym) = c.huff_decode(&self.tables[2], resume) else {
                            return self.stop(16, Self::starved(flags));
                        };
                        resume = false;
                        *dist = sym as u32;
                        if *dist < 16 {
                            self.len_codes[*counter as usize] = *dist as u8;
                            *counter += 1;
                            continue;
                        }
                        if *dist == 16 && *counter == 0 {
                            l = L::Forever(17, Status::Failed);
                            break;
                        }
                        *num_extra = [2u32, 3, 7][(*dist - 16) as usize];
                        l = L::LenCodesExtra;
                        break;
                    }
                    if matches!(l, L::LenCodes { .. }) {
                        let n0 = self.table_sizes[0] as usize;
                        let n1 = self.table_sizes[1] as usize;
                        if n0 + n1 != *counter as usize {
                            l = L::Forever(21, Status::Failed);
                            continue;
                        }
                        self.tables[0].code_size[..n0].copy_from_slice(&self.len_codes[..n0]);
                        self.tables[1].code_size[..n1]
                            .copy_from_slice(&self.len_codes[n0..n0 + n1]);
                        // r->m_type--: on to table 1, then table 0.
                        self.type_ = self.type_.wrapping_sub(1);
                        l = L::BuildTables;
                    }
                }
                L::LenCodesExtra => {
                    let Some(mut s) = c.get_bits(*num_extra) else {
                        return self.stop(18, Self::starved(flags));
                    };
                    s += [3u32, 3, 11][(*dist - 16) as usize];
                    let v = if *dist == 16 {
                        self.len_codes[*counter as usize - 1]
                    } else {
                        0
                    };
                    // TINFL_MEMSET past the end of the table's sizes stays inside m_len_codes
                    // (288 + 32 + 137 bytes), as in the C.
                    let start = *counter as usize;
                    let end = start + s as usize;
                    let lim = self.len_codes.len();
                    self.len_codes[start.min(lim)..end.min(lim)].fill(v);
                    *counter += s;
                    l = L::LenCodes { resume: false };
                }
                L::LitLoop => {
                    // for (;;) { for (;;) { ... } ... }: the inner literal loop.
                    loop {
                        if (c.input.len() - c.in_cur) < 4 || (c.out_end - c.out_cur) < 2 {
                            l = L::LitSlow { resume: false };
                            break;
                        }
                        if c.num_bits < 30 {
                            let w = u32::from_le_bytes([
                                c.input[c.in_cur],
                                c.input[c.in_cur + 1],
                                c.input[c.in_cur + 2],
                                c.input[c.in_cur + 3],
                            ]);
                            c.bit_buf |= u64::from(w) << c.num_bits;
                            c.in_cur += 4;
                            c.num_bits += 32;
                        }
                        let t0 = &self.tables[0];
                        let (sym, code_len) = fast_decode(t0, c.bit_buf);
                        *counter = sym as u32;
                        c.bit_buf >>= code_len;
                        c.num_bits -= code_len;
                        if *counter & 256 != 0 {
                            l = L::Length;
                            break;
                        }
                        let (sym2, code_len) = fast_decode(t0, c.bit_buf);
                        c.bit_buf >>= code_len;
                        c.num_bits -= code_len;
                        c.out[c.out_cur] = *counter as u8;
                        if sym2 & 256 != 0 {
                            c.out_cur += 1;
                            *counter = sym2 as u32;
                            l = L::Length;
                            break;
                        }
                        c.out[c.out_cur + 1] = sym2 as u8;
                        c.out_cur += 2;
                    }
                }
                L::LitSlow { resume } => {
                    let Some(sym) = c.huff_decode(&self.tables[0], resume) else {
                        return self.stop(23, Self::starved(flags));
                    };
                    *counter = sym as u32;
                    if *counter >= 256 {
                        l = L::Length;
                    } else {
                        l = L::LitSlowOut;
                    }
                }
                L::LitSlowOut => {
                    if c.out_cur >= c.out_end {
                        return self.stop(24, Status::HasMoreOutput);
                    }
                    c.out[c.out_cur] = *counter as u8;
                    c.out_cur += 1;
                    l = L::LitLoop;
                }
                L::Length => {
                    // `if ((counter &= 511) == 256) break;`, then the length's base and the
                    // number of its extra bits.
                    *counter &= 511;
                    if *counter == 256 {
                        l = L::EndBlock;
                        continue;
                    }
                    *num_extra = S_LENGTH_EXTRA[(*counter - 257) as usize];
                    *counter = S_LENGTH_BASE[(*counter - 257) as usize];
                    l = L::LengthExtra;
                }
                L::LengthExtra => {
                    // TINFL_GET_BITS(25, extra_bits, num_extra); a resume at 25 comes back here
                    // with the base and the count set.
                    if *num_extra != 0 {
                        let Some(extra_bits) = c.get_bits(*num_extra) else {
                            return self.stop(25, Self::starved(flags));
                        };
                        *counter += extra_bits;
                    }
                    l = L::Dist { resume: false };
                }
                L::Dist { resume } => {
                    let Some(sym) = c.huff_decode(&self.tables[1], resume) else {
                        return self.stop(26, Self::starved(flags));
                    };
                    *dist = sym as u32;
                    *num_extra = S_DIST_EXTRA[*dist as usize];
                    *dist = S_DIST_BASE[*dist as usize];
                    l = L::DistExtra;
                }
                L::DistExtra => {
                    if *num_extra != 0 {
                        let Some(extra_bits) = c.get_bits(*num_extra) else {
                            return self.stop(27, Self::starved(flags));
                        };
                        *dist += extra_bits;
                    }
                    *dist_from_out_buf_start = c.out_cur;
                    if (*dist as usize) > *dist_from_out_buf_start
                        && flags & TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF != 0
                    {
                        l = L::Forever(37, Status::Failed);
                        continue;
                    }
                    let src =
                        dist_from_out_buf_start.wrapping_sub(*dist as usize) & out_buf_size_mask;
                    let hi = if c.out_cur < src { src } else { c.out_cur };
                    if hi + *counter as usize > c.out_end {
                        l = L::CopySlow;
                        continue;
                    }
                    // The eight-byte copies are memcpy's of 8 bytes, which MSVC makes one load
                    // and one store: where a ring smaller than the distance puts the source just
                    // behind the output, they copy bytes the byte loop would have written first.
                    // counter is left as the C leaves it.
                    let mut s = src;
                    let mut cnt = *counter;
                    if cnt >= 9 && cnt <= *dist {
                        let s_end = s + (cnt & !7) as usize;
                        loop {
                            let mut word = [0u8; 8];
                            word.copy_from_slice(&c.out[s..s + 8]);
                            c.out[c.out_cur..c.out_cur + 8].copy_from_slice(&word);
                            c.out_cur += 8;
                            s += 8;
                            if s >= s_end {
                                break;
                            }
                        }
                        cnt &= 7;
                    }
                    while cnt > 2 {
                        c.out[c.out_cur] = c.out[s];
                        c.out[c.out_cur + 1] = c.out[s + 1];
                        c.out[c.out_cur + 2] = c.out[s + 2];
                        c.out_cur += 3;
                        s += 3;
                        cnt -= 3;
                    }
                    if cnt > 0 {
                        c.out[c.out_cur] = c.out[s];
                        if cnt > 1 {
                            c.out[c.out_cur + 1] = c.out[s + 1];
                        }
                        c.out_cur += cnt as usize;
                    }
                    *counter = cnt;
                    l = L::LitLoop;
                }
                L::CopySlow => {
                    // while (counter--) { while (out full) RETURN(53); ... }
                    let n = *counter;
                    *counter = counter.wrapping_sub(1);
                    if n == 0 {
                        l = L::LitLoop;
                        continue;
                    }
                    l = L::CopySlowOut;
                }
                L::CopySlowOut => {
                    if c.out_cur >= c.out_end {
                        return self.stop(53, Status::HasMoreOutput);
                    }
                    let i =
                        dist_from_out_buf_start.wrapping_sub(*dist as usize) & out_buf_size_mask;
                    *dist_from_out_buf_start = dist_from_out_buf_start.wrapping_add(1);
                    c.out[c.out_cur] = c.out[i];
                    c.out_cur += 1;
                    l = L::CopySlow;
                }
                L::EndBlock => {
                    // } while (!(r->m_final & 1));
                    l = if self.final_ & 1 == 0 {
                        L::Block
                    } else {
                        L::Tail
                    };
                }
                L::Tail => {
                    let n = c.num_bits & 7;
                    if !c.skip_bits(n) {
                        return self.stop(32, Self::starved(flags));
                    }
                    while c.in_cur > 0 && c.num_bits >= 8 {
                        c.in_cur -= 1;
                        c.num_bits -= 8;
                    }
                    c.bit_buf &= (1u64 << c.num_bits).wrapping_sub(1);
                    *counter = 0;
                    l = if flags & TINFL_FLAG_PARSE_ZLIB_HEADER != 0 {
                        L::Adler
                    } else {
                        L::Forever(34, Status::Done)
                    };
                }
                L::Adler => {
                    while *counter < 4 {
                        let s = if c.num_bits != 0 {
                            let Some(s) = c.get_bits(8) else {
                                return self.stop(41, Self::starved(flags));
                            };
                            s
                        } else {
                            let Some(s) = c.get_byte() else {
                                return self.stop(42, Self::starved(flags));
                            };
                            s
                        };
                        self.z_adler32 = (self.z_adler32 << 8) | s;
                        *counter += 1;
                    }
                    l = L::Forever(34, Status::Done);
                }
                L::Forever(state, status) => return self.stop(state, status),
            }
        }
    }

    /// One iteration of the table-building loop: `true` unless the code is over-subscribed
    /// (`TINFL_CR_RETURN_FOREVER(35, ...)`).
    fn build_table(&mut self, ty: usize) -> bool {
        let size = self.table_sizes[ty] as usize;
        let t = &mut self.tables[ty];
        let mut total_syms = [0u32; 16];
        t.look_up = [0; TINFL_FAST_LOOKUP_SIZE];
        t.tree = [0; TINFL_MAX_HUFF_SYMBOLS_0 * 2];
        for i in 0..size {
            total_syms[t.code_size[i] as usize] += 1;
        }
        let mut used_syms = 0u32;
        let mut total = 0u32;
        let mut next_code = [0u32; 17];
        for i in 1..=15 {
            used_syms += total_syms[i];
            total = (total + total_syms[i]) << 1;
            next_code[i + 1] = total;
        }
        if total != 65536 && used_syms > 1 {
            return false;
        }
        let mut tree_next: i32 = -1;
        for sym_index in 0..size {
            let code_size = u32::from(t.code_size[sym_index]);
            if code_size == 0 {
                continue;
            }
            let mut cur_code = next_code[code_size as usize];
            next_code[code_size as usize] += 1;
            let mut rev_code = 0u32;
            for _ in 0..code_size {
                rev_code = (rev_code << 1) | (cur_code & 1);
                cur_code >>= 1;
            }
            if code_size <= TINFL_FAST_LOOKUP_BITS {
                let k = ((code_size << 9) | sym_index as u32) as i16;
                while (rev_code as usize) < TINFL_FAST_LOOKUP_SIZE {
                    t.look_up[rev_code as usize] = k;
                    rev_code += 1 << code_size;
                }
                continue;
            }
            let slot = (rev_code as usize) & (TINFL_FAST_LOOKUP_SIZE - 1);
            let mut tree_cur = i32::from(t.look_up[slot]);
            if tree_cur == 0 {
                t.look_up[slot] = tree_next as i16;
                tree_cur = tree_next;
                tree_next -= 2;
            }
            rev_code >>= TINFL_FAST_LOOKUP_BITS - 1;
            let mut j = code_size;
            while j > TINFL_FAST_LOOKUP_BITS + 1 {
                rev_code >>= 1;
                tree_cur -= (rev_code & 1) as i32;
                let idx = (-tree_cur - 1) as usize;
                if t.tree[idx] == 0 {
                    t.tree[idx] = tree_next as i16;
                    tree_cur = tree_next;
                    tree_next -= 2;
                } else {
                    tree_cur = i32::from(t.tree[idx]);
                }
                j -= 1;
            }
            rev_code >>= 1;
            tree_cur -= (rev_code & 1) as i32;
            t.tree[(-tree_cur - 1) as usize] = sym_index as i16;
        }
        true
    }
}

/// The fast loop's table look-up: the symbol and its code length.
fn fast_decode(t: &HuffTable, bit_buf: u64) -> (i32, u32) {
    let mut sym2 = i32::from(t.look_up[(bit_buf & (TINFL_FAST_LOOKUP_SIZE as u64 - 1)) as usize]);
    if sym2 >= 0 {
        return (sym2, (sym2 >> 9) as u32);
    }
    let mut code_len = TINFL_FAST_LOOKUP_BITS;
    loop {
        sym2 = i32::from(t.tree[(!sym2 + ((bit_buf >> code_len) & 1) as i32) as usize]);
        code_len += 1;
        if sym2 >= 0 {
            return (sym2, code_len);
        }
    }
}
