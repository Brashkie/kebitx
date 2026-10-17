//! DEFLATE (RFC 1951) and zlib (RFC 1950) decoding: std only.
//!
//! Canonical Huffman decoding by code-length counts, the way zlib's
//! reference decoder `puff` does it: small and easy to check, and fast enough
//! for textures and a `.jar`. Every input is untrusted: every read is bounds
//! checked, every table is validated before use, and the output stops at a
//! caller-given limit (a few KB can claim gigabytes).

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

    /// Drop the bits left in the current byte (stored blocks start on a byte).
    fn align(&mut self) {
        let drop = self.count % 8;
        self.buf >>= drop;
        self.count -= drop;
    }
}

struct Huffman {
    /// How many codes of each length (index 0 unused).
    count: [u16; MAX_BITS + 1],
    /// Symbols ordered by code.
    symbol: Vec<u16>,
}

impl Huffman {
    /// From code lengths. `Ok(complete)` tells whether the code uses every
    /// bit pattern; an over-subscribed set is an error.
    fn new(lengths: &[u8]) -> Result<(Huffman, bool), InflateError> {
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
        let mut symbol = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbol[offs[l as usize] as usize] = sym as u16;
                offs[l as usize] += 1;
            }
        }
        Ok((Huffman { count, symbol }, left == 0))
    }

    fn decode(&self, bits: &mut Bits) -> Result<u16, InflateError> {
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
    let mut bits = Bits::new(data);
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = bits.bits(1)? == 1;
        match bits.bits(2)? {
            0 => stored(&mut bits, &mut out, limit)?,
            1 => {
                let (lit, dist) = fixed_tables();
                codes(&mut bits, &mut out, &lit, &dist, limit)?
            }
            2 => {
                let (lit, dist) = dynamic_tables(&mut bits)?;
                codes(&mut bits, &mut out, &lit, &dist, limit)?
            }
            _ => return Err(InflateError::Invalid("block type 3")),
        }
        if last {
            break;
        }
    }
    // bytes consumed: whole bytes read, minus the ones still in the buffer
    let used = bits.pos - (bits.count / 8) as usize;
    Ok((out, used))
}

fn stored(bits: &mut Bits, out: &mut Vec<u8>, limit: usize) -> Result<(), InflateError> {
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
    if out.len() + len > limit {
        return Err(InflateError::TooLarge);
    }
    out.extend_from_slice(chunk);
    bits.pos = start + len;
    bits.buf = 0;
    bits.count = 0;
    Ok(())
}

fn fixed_tables() -> (Huffman, Huffman) {
    let mut l = [0u8; 288];
    l[..144].fill(8);
    l[144..256].fill(9);
    l[256..280].fill(7);
    l[280..].fill(8);
    // the fixed tables are valid by construction
    let lit = Huffman::new(&l).map(|(h, _)| h).unwrap_or(Huffman {
        count: [0; 16],
        symbol: Vec::new(),
    });
    let dist = Huffman::new(&[5u8; 30]).map(|(h, _)| h).unwrap_or(Huffman {
        count: [0; 16],
        symbol: Vec::new(),
    });
    (lit, dist)
}

fn dynamic_tables(bits: &mut Bits) -> Result<(Huffman, Huffman), InflateError> {
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
    let (lencode, complete) = Huffman::new(&lengths[..19])?;
    if !complete {
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
    let (lit, complete) = Huffman::new(&lengths[..nlen])?;
    // an incomplete code is allowed only when every length is 0 or 1 (a
    // single code, or none: a block with no distances), as zlib allows
    if !complete && nlen != (lit.count[0] + lit.count[1]) as usize {
        return Err(InflateError::Invalid("incomplete literal code"));
    }
    let (dist, complete) = Huffman::new(&lengths[nlen..nlen + ndist])?;
    if !complete && ndist != (dist.count[0] + dist.count[1]) as usize {
        return Err(InflateError::Invalid("incomplete distance code"));
    }
    Ok((lit, dist))
}

fn codes(
    bits: &mut Bits,
    out: &mut Vec<u8>,
    lit: &Huffman,
    dist: &Huffman,
    limit: usize,
) -> Result<(), InflateError> {
    loop {
        let sym = lit.decode(bits)? as usize;
        match sym {
            0..=255 => {
                if out.len() >= limit {
                    return Err(InflateError::TooLarge);
                }
                out.push(sym as u8);
            }
            256 => return Ok(()),
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
                if back > out.len() {
                    return Err(InflateError::Invalid("distance before the start"));
                }
                if out.len() + len > limit {
                    return Err(InflateError::TooLarge);
                }
                let from = out.len() - back;
                for k in 0..len {
                    let b = out[from + k];
                    out.push(b);
                }
            }
        }
    }
}

/// A zlib stream (header, DEFLATE data, Adler-32) -> bytes, at most `limit`.
pub fn zlib_decompress(data: &[u8], limit: usize) -> Result<Vec<u8>, InflateError> {
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
    let (out, used) = inflate(&data[2..], limit)?;
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
