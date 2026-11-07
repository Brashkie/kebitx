//! DEFLATE (RFC 1951) and zlib (RFC 1950) decoding: std only.
//!
//! Canonical Huffman decoding, with two paths through the same tables:
//!
//!   - the careful one, a symbol at a time with every check, and `puff`'s
//!     bit-by-bit walk for long codes and the end of a stream: it decides
//!     every error;
//!   - the fast loop: 64-bit refills, up to three literals a round and copies
//!     by whole words. When anything is not simple it puts the state back as
//!     it was before that round and lets the careful path decide. So it cannot
//!     give a verdict of its own, and the tests compare both on damaged input.
//!
//! Measured on the game's 11 383 class files (one core, CRC-32 included): 187
//! MB/s before the fast loop, 313 MB/s now. The tables grow by doubling and are
//! kept per thread, since building them was 28 % of the decoding; copies go by
//! whole words with no branch between the first ones. The same loop in Zig,
//! without bounds checks, gave 4 % more, and libdeflate (C) was within 4 % on
//! real commands: neither was worth keeping.
//!
//! Every input is untrusted: every read is bounds checked, every table is
//! validated before use, and the output stops at a caller-given limit (a few
//! KB can claim gigabytes).

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InflateError {
    /// The data ends before the stream does.
    Truncated,
    /// A table, length or distance the format does not allow.
    Invalid(&'static str),
    /// The output would pass the caller's limit.
    TooLarge,
}

impl fmt::Display for InflateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InflateError::Truncated => write!(f, "compressed data ends early"),
            InflateError::Invalid(why) => write!(f, "invalid compressed data: {why}"),
            InflateError::TooLarge => write!(f, "decompressed data larger than allowed"),
        }
    }
}

const MAX_BITS: usize = 15;

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u64,
    count: u32,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Bits {
            data,
            pos: 0,
            buf: 0,
            count: 0,
        }
    }

    fn need(&mut self, n: u32) -> Result<(), InflateError> {
        while self.count < n {
            let b = *self.data.get(self.pos).ok_or(InflateError::Truncated)?;
            self.pos += 1;
            self.buf |= (b as u64) << self.count;
            self.count += 8;
        }
        Ok(())
    }

    fn bits(&mut self, n: u32) -> Result<u32, InflateError> {
        if n == 0 {
            return Ok(0);
        }
        self.need(n)?;
        let v = (self.buf & ((1u64 << n) - 1)) as u32;
        self.buf >>= n;
        self.count -= n;
        Ok(v)
    }

    /// Fill the buffer with whatever input is left, up to 56 bits: never an
    /// error (the end of the data is found by the exact reads).
    #[inline]
    fn fill(&mut self) {
        while self.count <= 48 {
            let Some(&b) = self.data.get(self.pos) else {
                return;
            };
            self.pos += 1;
            self.buf |= (b as u64) << self.count;
            self.count += 8;
        }
    }

    /// Drop the bits left in the current byte (stored blocks start on a byte).
    fn align(&mut self) {
        let drop = self.count % 8;
        self.buf >>= drop;
        self.count -= drop;
    }
}

/// Bits looked up at once: literals and lengths, distances. Longer codes are
/// rare by construction, and take the careful path.
const LIT_BITS: u32 = 11;
const DIST_BITS: u32 = 9;
/// Enough for the code-length code, whose codes have at most 7 bits.
const LEN_BITS: u32 = 7;

/// A table entry: bits 0-3 the code's length (0: no code of at most the
/// table's bits starts here), bits 4-7 the extra bits after it, bits 8-16
/// the symbol, bits 17-31 the base value of a length or a distance.
const NO_CODE: u32 = 0x1ff << 8;

// What follows a symbol: its base value and extra bits, when it has them.
fn no_values(_: usize) -> (u32, u32) {
    (0, 0)
}

fn length_values(sym: usize) -> (u32, u32) {
    match sym.checked_sub(257) {
        Some(s) if s < 29 => (LEN_BASE[s] as u32, LEN_EXTRA[s] as u32),
        _ => (0, 0),
    }
}

fn distance_values(sym: usize) -> (u32, u32) {
    if sym < 30 {
        (DIST_BASE[sym] as u32, DIST_EXTRA[sym] as u32)
    } else {
        (0, 0)
    }
}

const MAX_SYMBOLS: usize = 320;

/// A canonical Huffman code, with a table of `N` (a power of two) entries.
struct Huffman<const N: usize> {
    /// How many codes of each length (index 0 unused).
    count: [u16; MAX_BITS + 1],
    /// Symbols ordered by code.
    symbol: [u16; MAX_SYMBOLS],
    /// The next input bits (as read, least significant first) -> an entry.
    table: [u32; N],
}

/// The `len` low bits of `code`, reversed: DEFLATE stores Huffman codes from
/// their most significant bit, into a stream read from the least.
fn reverse(code: u32, len: u32) -> u32 {
    code.reverse_bits() >> (32 - len)
}

impl<const N: usize> Huffman<N> {
    fn empty() -> Self {
        Huffman {
            count: [0; MAX_BITS + 1],
            symbol: [0; MAX_SYMBOLS],
            table: [NO_CODE; N],
        }
    }

    /// From code lengths. `Ok(complete)` tells whether the code uses every
    /// bit pattern; an over-subscribed set is an error.
    #[inline]
    fn build(
        &mut self,
        lengths: &[u8],
        values: impl Fn(usize) -> (u32, u32),
    ) -> Result<bool, InflateError> {
        let mut count = [0u16; MAX_BITS + 1];
        for &l in lengths {
            count[l as usize] += 1;
        }
        let mut left: i32 = 1;
        for &c in &count[1..] {
            left <<= 1;
            left -= c as i32;
            if left < 0 {
                return Err(InflateError::Invalid("over-subscribed code"));
            }
        }
        let mut offs = [0u16; MAX_BITS + 1];
        for len in 1..MAX_BITS {
            offs[len + 1] = offs[len] + count[len];
        }
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                self.symbol[offs[l as usize] as usize] = sym as u16;
                offs[l as usize] += 1;
            }
        }
        self.count = count;
        // canonical codes, shortest first and by symbol within a length: the
        // order `symbol` holds and `decode_slow` walks. A code of `len` bits
        // repeats every `1 << len` entries, so the table grows by doubling:
        // each length writes its codes once, then the part done is copied.
        // Whatever no code reaches stays NO_CODE: longer codes, and holes.
        let table_bits = N.trailing_zeros();
        self.table[..2].fill(NO_CODE);
        let (mut code, mut at) = (0u32, 0usize);
        for len in 1..=MAX_BITS as u32 {
            let codes = count[len as usize] as usize;
            if len > table_bits {
                break;
            }
            for &sym in &self.symbol[at..at + codes] {
                let (base, extra) = values(sym as usize);
                self.table[reverse(code, len) as usize] =
                    base << 17 | (sym as u32) << 8 | extra << 4 | len;
                code += 1;
            }
            at += codes;
            code <<= 1;
            if len < table_bits {
                self.table.copy_within(0..1 << len, 1 << len);
            }
        }
        Ok(left == 0)
    }

    /// One symbol, with every check.
    #[inline]
    fn decode(&self, bits: &mut Bits) -> Result<u16, InflateError> {
        if bits.count < N.trailing_zeros() {
            bits.fill();
        }
        let entry = self.table[bits.buf as usize & (N - 1)];
        let len = entry & 15;
        if len != 0 && len <= bits.count {
            bits.buf >>= len;
            bits.count -= len;
            return Ok((entry >> 8 & 0x1ff) as u16);
        }
        self.decode_slow(bits)
    }

    /// One bit at a time, as `puff`: long codes, and the end of the stream.
    fn decode_slow(&self, bits: &mut Bits) -> Result<u16, InflateError> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..=MAX_BITS {
            code |= bits.bits(1)? as i32;
            let count = self.count[len] as i32;
            if code - first < count {
                return Ok(self.symbol[(index + code - first) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err(InflateError::Invalid("no code matches"))
    }
}

type LitTable = Huffman<{ 1 << LIT_BITS }>;
type DistTable = Huffman<{ 1 << DIST_BITS }>;

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Raw DEFLATE data -> bytes, at most `limit` of them. Returns the output and
/// how many input bytes the stream used.
pub fn inflate(data: &[u8], limit: usize) -> Result<(Vec<u8>, usize), InflateError> {
    decode_stream::<true>(data, limit)
}

/// The same decoder, one symbol at a time with every check: what the fast
/// loop must always agree with. The tests compare them.
pub fn inflate_careful(data: &[u8], limit: usize) -> Result<(Vec<u8>, usize), InflateError> {
    decode_stream::<false>(data, limit)
}

/// Room after the output's end: three literals, a longest match, and what a
/// copy by whole words may write past it. Never part of the result.
const SLACK: usize = 3 + 258 + 48;

/// The output: `buf` is written by index up to `at`, and cut to it at the end.
struct Output {
    buf: Vec<u8>,
    at: usize,
    limit: usize,
}

impl Output {
    /// Room for one more round of the fast loop, or one careful symbol.
    fn make_room(&mut self) {
        let wanted = self.at + SLACK;
        if wanted > self.buf.len() {
            let grown = self.buf.len().saturating_mul(2);
            self.buf
                .resize(grown.min(self.limit.saturating_add(SLACK)).max(wanted), 0);
        }
    }
}

thread_local! {
    /// The tables of a stream: they hold nothing between calls, and are 10 KB.
    static TABLES: std::cell::RefCell<Box<(LitTable, DistTable)>> =
        std::cell::RefCell::new(Box::new((LitTable::empty(), DistTable::empty())));
}

fn decode_stream<const FAST: bool>(
    data: &[u8],
    limit: usize,
) -> Result<(Vec<u8>, usize), InflateError> {
    let mut bits = Bits::new(data);
    // room for typical ratios up front; a claim beyond the limit is never
    // reserved (a hostile stream cannot make this allocate more)
    let start = limit.min(data.len().saturating_mul(4) + 1024) + SLACK;
    let mut out = Output {
        buf: vec![0; start],
        at: 0,
        limit,
    };
    TABLES.with(|tables| {
        let mut tables = tables.borrow_mut();
        let (lit, dist) = &mut **tables;
        loop {
            let last = bits.bits(1)? == 1;
            match bits.bits(2)? {
                0 => stored(&mut bits, &mut out)?,
                1 => {
                    fixed_tables(lit, dist);
                    codes::<FAST>(&mut bits, &mut out, lit, dist)?
                }
                2 => {
                    dynamic_tables(&mut bits, lit, dist)?;
                    codes::<FAST>(&mut bits, &mut out, lit, dist)?
                }
                _ => return Err(InflateError::Invalid("block type 3")),
            }
            if last {
                break;
            }
        }
        Ok(())
    })?;
    // bytes consumed: whole bytes read, minus the ones still in the buffer
    let used = bits.pos - (bits.count / 8) as usize;
    let at = out.at;
    let mut result = out.buf;
    result.truncate(at);
    Ok((result, used))
}

fn stored(bits: &mut Bits, out: &mut Output) -> Result<(), InflateError> {
    bits.align();
    let len = bits.bits(16)? as usize;
    let nlen = bits.bits(16)? as usize;
    if len != !nlen & 0xffff {
        return Err(InflateError::Invalid("stored length check"));
    }
    // the buffer is empty after two whole 16-bit reads from a byte boundary
    let start = bits.pos - (bits.count / 8) as usize;
    let chunk = bits
        .data
        .get(start..start + len)
        .ok_or(InflateError::Truncated)?;
    if out.at + len > out.limit {
        return Err(InflateError::TooLarge);
    }
    if out.at + len + SLACK > out.buf.len() {
        out.buf.resize(out.at + len + SLACK, 0);
    }
    out.buf[out.at..out.at + len].copy_from_slice(chunk);
    out.at += len;
    bits.pos = start + len;
    bits.buf = 0;
    bits.count = 0;
    Ok(())
}

fn fixed_tables(lit: &mut LitTable, dist: &mut DistTable) {
    let mut l = [0u8; 288];
    l[..144].fill(8);
    l[144..256].fill(9);
    l[256..280].fill(7);
    l[280..].fill(8);
    // both are valid by construction: only an over-subscribed code fails
    let _ = lit.build(&l, length_values);
    let _ = dist.build(&[5u8; 30], distance_values);
}

fn dynamic_tables(
    bits: &mut Bits,
    lit: &mut LitTable,
    dist: &mut DistTable,
) -> Result<(), InflateError> {
    const ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let nlen = bits.bits(5)? as usize + 257;
    let ndist = bits.bits(5)? as usize + 1;
    let ncode = bits.bits(4)? as usize + 4;
    if nlen > 286 || ndist > 30 {
        return Err(InflateError::Invalid("too many codes"));
    }
    let mut lengths = [0u8; 320];
    for &i in &ORDER[..ncode] {
        lengths[i] = bits.bits(3)? as u8;
    }
    let mut lencode = Huffman::<{ 1 << LEN_BITS }>::empty();
    if !lencode.build(&lengths[..19], no_values)? {
        return Err(InflateError::Invalid("incomplete code-length code"));
    }
    let mut i = 0;
    while i < nlen + ndist {
        let sym = lencode.decode(bits)?;
        if sym < 16 {
            lengths[i] = sym as u8;
            i += 1;
            continue;
        }
        let (value, repeat) = match sym {
            16 => {
                if i == 0 {
                    return Err(InflateError::Invalid("repeat with no previous length"));
                }
                (lengths[i - 1], 3 + bits.bits(2)? as usize)
            }
            17 => (0, 3 + bits.bits(3)? as usize),
            _ => (0, 11 + bits.bits(7)? as usize),
        };
        if i + repeat > nlen + ndist {
            return Err(InflateError::Invalid("lengths overrun"));
        }
        lengths[i..i + repeat].fill(value);
        i += repeat;
    }
    if lengths[256] == 0 {
        return Err(InflateError::Invalid("no end-of-block code"));
    }
    let complete = lit.build(&lengths[..nlen], length_values)?;
    // an incomplete code is allowed only when every length is 0 or 1 (a
    // single code, or none: a block with no distances), as zlib allows
    if !complete && nlen != (lit.count[0] + lit.count[1]) as usize {
        return Err(InflateError::Invalid("incomplete literal code"));
    }
    let complete = dist.build(&lengths[nlen..nlen + ndist], distance_values)?;
    if !complete && ndist != (dist.count[0] + dist.count[1]) as usize {
        return Err(InflateError::Invalid("incomplete distance code"));
    }
    Ok(())
}

/// A block's symbols: the fast loop for as long as it can go, then one symbol
/// with every check, and again.
fn codes<const FAST: bool>(
    bits: &mut Bits,
    out: &mut Output,
    lit: &LitTable,
    dist: &DistTable,
) -> Result<(), InflateError> {
    loop {
        out.make_room();
        if FAST {
            fast_loop(bits, out, lit, dist);
            out.make_room();
        }
        if careful_symbol(bits, out, lit, dist)? {
            return Ok(());
        }
    }
}

/// Decodes while it is simple: enough input for whole words, codes in the
/// tables, valid symbols. Anything else (the end of the block, a long code,
/// an error, the output's limit) leaves the state as it was before that round,
/// for `careful_symbol` to decide what it means. So this loop has no error to
/// get wrong: every verdict is the careful path's.
fn fast_loop(bits: &mut Bits, out: &mut Output, lit: &LitTable, dist: &DistTable) {
    const LIT_MASK: u64 = (1 << LIT_BITS) - 1;
    const DIST_MASK: u64 = (1 << DIST_BITS) - 1;
    let data = bits.data;
    let limit = out.limit;
    let buffer = out.buf.as_mut_slice();
    let last_round = buffer.len() - SLACK;
    let (mut buf, mut count, mut pos, mut at) = (bits.buf, bits.count, bits.pos, out.at);
    let lit_table: &[u32; 1 << LIT_BITS] = &lit.table;
    let dist_table: &[u32; 1 << DIST_BITS] = &dist.table;
    loop {
        if at > last_round {
            break;
        }
        // a round that cannot finish goes back to here
        let state = (buf, count, pos, at);
        macro_rules! give_up {
            () => {{
                (buf, count, pos, at) = state;
                break;
            }};
        }
        macro_rules! refill {
            () => {
                let Some(Ok(word)) = data.get(pos..pos + 8).map(<[u8; 8]>::try_from) else {
                    give_up!();
                };
                buf |= u64::from_le_bytes(word) << count;
                pos += ((63 - count) >> 3) as usize;
                count |= 56;
            };
        }
        refill!();
        // at least 56 bits: up to three literals of at most 11 bits each
        let mut entry = lit_table[(buf & LIT_MASK) as usize];
        if entry & 1 << 16 == 0 {
            buffer[at] = (entry >> 8) as u8;
            at += 1;
            buf >>= entry & 15;
            count -= entry & 15;
            entry = lit_table[(buf & LIT_MASK) as usize];
            if entry & 1 << 16 == 0 {
                buffer[at] = (entry >> 8) as u8;
                at += 1;
                buf >>= entry & 15;
                count -= entry & 15;
                entry = lit_table[(buf & LIT_MASK) as usize];
                if entry & 1 << 16 == 0 {
                    buffer[at] = (entry >> 8) as u8;
                    at += 1;
                    buf >>= entry & 15;
                    count -= entry & 15;
                    if at > limit {
                        give_up!();
                    }
                    continue;
                }
            }
            // a match needs up to 38 bits: 11 + 5 + 9 + 13
            refill!();
        }
        if entry >> 17 == 0 {
            give_up!(); // the end of the block, a long code, or no such symbol
        }
        buf >>= entry & 15;
        count -= entry & 15;
        let extra = entry >> 4 & 15;
        let len = (entry >> 17) as usize + (buf & ((1u64 << extra) - 1)) as usize;
        buf >>= extra;
        count -= extra;

        let entry = dist_table[(buf & DIST_MASK) as usize];
        if entry >> 17 == 0 {
            give_up!(); // a long code, or none
        }
        buf >>= entry & 15;
        count -= entry & 15;
        let extra = entry >> 4 & 15;
        let back = (entry >> 17) as usize + (buf & ((1u64 << extra) - 1)) as usize;
        buf >>= extra;
        count -= extra;

        let end = at + len;
        if back > at || end > limit {
            give_up!();
        }
        // Words overlap safely when they are a whole period apart. A period
        // under 8 bytes is first written byte by byte up to the first multiple
        // of it that is at least 8: from there on, words at that distance
        // repeat it.
        let (mut from, mut to) = if back >= 8 {
            (at - back, at)
        } else {
            let step = STEP[back];
            for k in at..at + step {
                buffer[k] = buffer[k - back];
            }
            (at, at + step)
        };
        // most matches are short: words with no branch between them, and the
        // slack takes what passes the end
        for _ in 0..4 {
            copy_word(buffer, from, to);
            from += 8;
            to += 8;
        }
        while to < end {
            copy_word(buffer, from, to);
            from += 8;
            to += 8;
        }
        at = end;
    }
    // only what `count` says is in the buffer: a refill leaves more above it,
    // and the careful path adds its bytes right at `count`
    bits.buf = if count == 0 {
        0
    } else {
        buf & (u64::MAX >> (64 - count))
    };
    bits.count = count;
    bits.pos = pos;
    out.at = at;
}

/// The first multiple of each period under 8 that is at least 8.
const STEP: [usize; 8] = [8, 8, 8, 9, 8, 10, 12, 14];

#[inline(always)]
fn copy_word(buffer: &mut [u8], from: usize, to: usize) {
    let mut word = [0u8; 8];
    word.copy_from_slice(&buffer[from..from + 8]);
    buffer[to..to + 8].copy_from_slice(&word);
}

/// One symbol, with every check. `true` at the end of the block.
fn careful_symbol(
    bits: &mut Bits,
    out: &mut Output,
    lit: &LitTable,
    dist: &DistTable,
) -> Result<bool, InflateError> {
    let sym = lit.decode(bits)? as usize;
    match sym {
        0..=255 => {
            if out.at >= out.limit {
                return Err(InflateError::TooLarge);
            }
            out.buf[out.at] = sym as u8;
            out.at += 1;
        }
        256 => return Ok(true),
        _ => {
            let s = sym - 257;
            if s >= 29 {
                return Err(InflateError::Invalid("length symbol"));
            }
            let len = LEN_BASE[s] as usize + bits.bits(LEN_EXTRA[s] as u32)? as usize;
            let d = dist.decode(bits)? as usize;
            if d >= 30 {
                return Err(InflateError::Invalid("distance symbol"));
            }
            let back = DIST_BASE[d] as usize + bits.bits(DIST_EXTRA[d] as u32)? as usize;
            if back > out.at {
                return Err(InflateError::Invalid("distance before the start"));
            }
            if out.at + len > out.limit {
                return Err(InflateError::TooLarge);
            }
            for k in out.at..out.at + len {
                out.buf[k] = out.buf[k - back];
            }
            out.at += len;
        }
    }
    Ok(false)
}

/// A zlib stream (header, DEFLATE data, Adler-32) -> bytes, at most `limit`.
pub fn zlib_decompress(data: &[u8], limit: usize) -> Result<Vec<u8>, InflateError> {
    zlib_with(data, limit, inflate)
}

/// The same through the careful path: see `inflate_careful`.
pub fn zlib_decompress_careful(data: &[u8], limit: usize) -> Result<Vec<u8>, InflateError> {
    zlib_with(data, limit, inflate_careful)
}

type Deflate = fn(&[u8], usize) -> Result<(Vec<u8>, usize), InflateError>;

fn zlib_with(data: &[u8], limit: usize, deflate: Deflate) -> Result<Vec<u8>, InflateError> {
    let (&cmf, &flg) = (
        data.first().ok_or(InflateError::Truncated)?,
        data.get(1).ok_or(InflateError::Truncated)?,
    );
    if cmf & 0x0f != 8 || !((cmf as u16) << 8 | flg as u16).is_multiple_of(31) {
        return Err(InflateError::Invalid("zlib header"));
    }
    if flg & 0x20 != 0 {
        return Err(InflateError::Invalid("zlib preset dictionary"));
    }
    let (out, used) = deflate(&data[2..], limit)?;
    let tail = data
        .get(2 + used..2 + used + 4)
        .ok_or(InflateError::Truncated)?;
    if u32::from_be_bytes([tail[0], tail[1], tail[2], tail[3]]) != super::adler32(&out) {
        return Err(InflateError::Invalid("zlib checksum"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_block() {
        assert_eq!(
            inflate(&[0x01, 0x03, 0x00, 0xfc, 0xff, b'a', b'b', b'c'], 100)
                .unwrap()
                .0,
            b"abc"
        );
    }

    #[test]
    fn hostile_input_is_an_error_never_a_panic() {
        assert_eq!(inflate(&[], 10), Err(InflateError::Truncated));
        assert!(inflate(&[0x07], 10).is_err(), "block type 3");
        // a stored block larger than the limit
        assert_eq!(
            inflate(&[0x01, 0x03, 0x00, 0xfc, 0xff, b'a', b'b', b'c'], 2),
            Err(InflateError::TooLarge)
        );
        // a real stream: every prefix and many one-byte corruptions
        let text: Vec<u8> = (0..3000u32)
            .map(|i| b"hello deflate world "[(i * 7 % 20) as usize])
            .collect();
        let packed = super::super::deflate(&text);
        assert_eq!(inflate(&packed, 10_000).unwrap().0, text);
        assert_eq!(inflate(&packed, 100), Err(InflateError::TooLarge));
        for cut in 0..packed.len() {
            let _ = inflate(&packed[..cut], 10_000);
        }
        for i in (0..packed.len()).step_by(3) {
            for b in [0x00, 0xff, 0x55] {
                let mut d = packed.clone();
                d[i] = b;
                let _ = inflate(&d, 10_000);
            }
        }
    }
}
