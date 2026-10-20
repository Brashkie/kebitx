//! Reading and writing the binary formats packs are made of, std only:
//! DEFLATE/zlib (`inflate`), PNG and TGA pixels (`png`, `tga`), and ZIP
//! archives such as the Java game's `.jar` (`zip`).
//!
//! All of it reads untrusted files: sizes are checked before anything is
//! allocated, every limit is explicit, and a damaged file is an error.

pub mod inflate;
pub mod png;
pub mod tga;
pub mod zip;

pub use inflate::{inflate, zlib_decompress, InflateError};

/// CRC-32 (IEEE), as PNG and ZIP use it.
pub fn crc32(data: &[u8]) -> u32 {
    crc32_update(0, data)
}

/// Continue a CRC-32 over more bytes. Eight bytes per step ("slicing by
/// 8", eight tables of 1 KB): measured at about 4x the byte-at-a-time loop
/// on the game's class files; the bytes after the last full eight go one
/// at a time.
pub fn crc32_update(crc: u32, data: &[u8]) -> u32 {
    let t = &CRC_TABLES;
    let mut c = !crc;
    let (chunks, rest) = data.as_chunks::<8>();
    for b in chunks {
        let lo = c ^ u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let hi = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
        c = t[7][(lo & 0xff) as usize]
            ^ t[6][(lo >> 8 & 0xff) as usize]
            ^ t[5][(lo >> 16 & 0xff) as usize]
            ^ t[4][(lo >> 24) as usize]
            ^ t[3][(hi & 0xff) as usize]
            ^ t[2][(hi >> 8 & 0xff) as usize]
            ^ t[1][(hi >> 16 & 0xff) as usize]
            ^ t[0][(hi >> 24) as usize];
    }
    for &b in rest {
        c = t[0][((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    !c
}

/// `CRC_TABLES[k][i]`: the CRC of byte `i` followed by `k` zero bytes, so
/// eight bytes combine in one step. Computed at compile time.
const CRC_TABLES: [[u32; 256]; 8] = {
    let mut t = [[0u32; 256]; 8];
    t[0] = CRC_TABLE;
    let mut k = 1;
    while k < 8 {
        let mut i = 0;
        while i < 256 {
            let prev = t[k - 1][i];
            t[k][i] = (prev >> 8) ^ t[0][(prev & 0xff) as usize];
            i += 1;
        }
        k += 1;
    }
    t
};

/// The CRC-32 table, computed at compile time.
const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

/// Adler-32, the zlib checksum.
pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

struct BitWriter {
    out: Vec<u8>,
    buf: u64,
    count: u32,
}

impl BitWriter {
    fn put(&mut self, value: u32, n: u32) {
        self.buf |= (value as u64) << self.count;
        self.count += n;
        while self.count >= 8 {
            self.out.push(self.buf as u8);
            self.buf >>= 8;
            self.count -= 8;
        }
    }
    /// Huffman codes are sent most significant bit first.
    fn put_code(&mut self, code: u32, len: u32) {
        let rev = code.reverse_bits() >> (32 - len);
        self.put(rev, len);
    }
    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 {
            self.out.push(self.buf as u8);
        }
        self.out
    }
}

fn fixed_literal(w: &mut BitWriter, sym: u32) {
    match sym {
        0..=143 => w.put_code(0x30 + sym, 8),
        144..=255 => w.put_code(0x190 + sym - 144, 9),
        256..=279 => w.put_code(sym - 256, 7),
        _ => w.put_code(0xc0 + sym - 280, 8),
    }
}

const LEN_BASE: [u32; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u32; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Raw DEFLATE: one block with the fixed Huffman code, LZ77 matches found by
/// a hash chain. Not the smallest output possible, always a valid one.
pub fn deflate(data: &[u8]) -> Vec<u8> {
    const WINDOW: usize = 32768;
    const HASH: usize = 1 << 15;
    const CHAIN: usize = 64;
    let mut w = BitWriter {
        out: Vec::with_capacity(data.len() / 2 + 16),
        buf: 0,
        count: 0,
    };
    w.put(1, 1); // last block
    w.put(1, 2); // fixed Huffman
    let hash = |i: usize| {
        ((data[i] as usize) << 10 ^ (data[i + 1] as usize) << 5 ^ data[i + 2] as usize) & (HASH - 1)
    };
    let mut head = vec![usize::MAX; HASH];
    let mut prev = vec![usize::MAX; data.len()];
    let mut i = 0;
    let insert = |head: &mut Vec<usize>, prev: &mut Vec<usize>, p: usize| {
        if p + 2 < data.len() {
            let h = hash(p);
            prev[p] = head[h];
            head[h] = p;
        }
    };
    while i < data.len() {
        let (mut best_len, mut best_dist) = (0, 0);
        if i + 2 < data.len() {
            let mut cand = head[hash(i)];
            let mut steps = 0;
            while cand != usize::MAX && i - cand <= WINDOW && steps < CHAIN {
                let max = (data.len() - i).min(258);
                let mut l = 0;
                while l < max && data[cand + l] == data[i + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_dist = i - cand;
                    if l == max {
                        break;
                    }
                }
                cand = prev[cand];
                steps += 1;
            }
        }
        if best_len >= 3 {
            let li = LEN_BASE
                .iter()
                .rposition(|&b| b as usize <= best_len)
                .unwrap_or(0);
            fixed_literal(&mut w, 257 + li as u32);
            w.put(best_len as u32 - LEN_BASE[li], LEN_EXTRA[li]);
            let di = DIST_BASE
                .iter()
                .rposition(|&b| b as usize <= best_dist)
                .unwrap_or(0);
            w.put_code(di as u32, 5);
            w.put(best_dist as u32 - DIST_BASE[di], DIST_EXTRA[di]);
            for p in i..i + best_len {
                insert(&mut head, &mut prev, p);
            }
            i += best_len;
        } else {
            fixed_literal(&mut w, data[i] as u32);
            insert(&mut head, &mut prev, i);
            i += 1;
        }
    }
    fixed_literal(&mut w, 256);
    w.finish()
}

/// A zlib stream of `data`.
pub fn zlib_compress(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    out.extend(deflate(data));
    out.extend(adler32(data).to_be_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_known_values() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32_update(crc32(b"1234"), b"56789"), 0xcbf4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
    }

    #[test]
    fn compress_then_decompress_is_the_identity() {
        let mut cases: Vec<Vec<u8>> = vec![vec![], b"a".to_vec(), vec![0u8; 100_000]];
        cases.push(
            (0..70_000u32)
                .map(|i| (i.wrapping_mul(2_654_435_761u32) >> 24) as u8)
                .collect(),
        );
        cases.push(
            (0..50_000u32)
                .map(|i| b"the quick brown fox "[(i % 20) as usize])
                .collect(),
        );
        for c in cases {
            let z = zlib_compress(&c);
            assert_eq!(zlib_decompress(&z, c.len() + 1).unwrap(), c);
        }
    }
}
