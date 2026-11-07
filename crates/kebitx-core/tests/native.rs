//! Every path through the codecs against the others, on the same input.
//!
//! `fixtures/deflate/streams.bin` holds 150 DEFLATE and zlib streams made by
//! another compressor (Zig's, with dynamic Huffman codes, as real files have),
//! each with its size and the CRC-32 of its data. The decoder's fast loop must
//! answer exactly what its careful path answers: the same bytes, the same
//! input used and the same error, on valid streams, on every truncation, on
//! corrupted bytes and on random data. Built with `--features native`,
//! `png::unfilter` goes through Zig, and must give the bytes of the Rust path.

use kebitx_core::codec::inflate::{inflate_careful, zlib_decompress_careful};
use kebitx_core::codec::png::{unfilter, unfilter_rust};
use kebitx_core::codec::{crc32, inflate, zlib_decompress, NATIVE};

struct Stream {
    zlib: bool,
    size: usize,
    crc: u32,
    packed: Vec<u8>,
}

fn streams() -> Vec<Stream> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/deflate/streams.bin"
    );
    let b = std::fs::read(path).expect("fixtures/deflate/streams.bin");
    let le = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    let mut out = Vec::new();
    let mut at = 4;
    for _ in 0..le(0) {
        let (zlib, size, crc, len) = (
            b[at] == 1,
            le(at + 1) as usize,
            le(at + 5),
            le(at + 9) as usize,
        );
        out.push(Stream {
            zlib,
            size,
            crc,
            packed: b[at + 13..at + 13 + len].to_vec(),
        });
        at += 13 + len;
    }
    out
}

/// Both paths give the very same answer, and it is returned.
fn agree(s: &Stream, data: &[u8], limit: usize, what: &str) -> Result<Vec<u8>, String> {
    if s.zlib {
        let fast = zlib_decompress(data, limit);
        assert_eq!(fast, zlib_decompress_careful(data, limit), "{what}");
        fast.map_err(|e| e.to_string())
    } else {
        let fast = inflate(data, limit);
        assert_eq!(fast, inflate_careful(data, limit), "{what}");
        fast.map(|(o, _)| o).map_err(|e| e.to_string())
    }
}

#[test]
fn every_stream_decodes_to_its_data_on_both_paths() {
    let all = streams();
    assert_eq!(all.len(), 150);
    for (i, s) in all.iter().enumerate() {
        let what = format!("stream {i}");
        let out = agree(s, &s.packed, s.size, &what).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert_eq!((out.len(), crc32(&out)), (s.size, s.crc), "{what}");
        // room for less than the data needs is refused, at every small margin
        for short in 1..=s.size.min(300) {
            if short < 8 || short % 37 == 0 {
                assert!(
                    agree(s, &s.packed, s.size - short, &what).is_err(),
                    "{what}"
                );
            }
        }
        // room for more changes nothing
        assert_eq!(agree(s, &s.packed, s.size + 1000, &what).unwrap(), out);
    }
}

/// Damaged input: prefixes of the streams, and bytes changed across them.
#[test]
fn damaged_streams_get_the_same_answer_on_both_paths() {
    let all = streams();
    let (mut decoded, mut refused) = (0, 0);
    for (i, s) in all.iter().enumerate().filter(|(i, _)| i % 7 == 0) {
        let mut cases: Vec<Vec<u8>> = (0..s.packed.len())
            .step_by((s.packed.len() / 40).max(1))
            .map(|n| s.packed[..n].to_vec())
            .collect();
        for at in (0..s.packed.len()).step_by((s.packed.len() / 25).max(1)) {
            for v in [0x00, 0xff, s.packed[at] ^ 0x10] {
                let mut d = s.packed.clone();
                d[at] = v;
                cases.push(d);
            }
        }
        for d in cases {
            for limit in [s.size + 64, s.size / 2] {
                match agree(s, &d, limit, &format!("stream {i}")) {
                    Ok(_) => decoded += 1,
                    Err(_) => refused += 1,
                }
            }
        }
    }
    eprintln!("{decoded} damaged inputs decoded, {refused} refused, always the same way");
    assert!(decoded > 0 && refused > 0);
}

/// Random data is mostly invalid, in every way a stream can be. A plain
/// generator: the same cases on every run.
#[test]
fn random_data_gets_the_same_answer_on_both_paths() {
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let s = Stream {
        zlib: false,
        size: 0,
        crc: 0,
        packed: Vec::new(),
    };
    for case in 0..20_000 {
        let len = (next() % 400) as usize;
        let mut d: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        // a fixed-code block gets much further than random block types do
        if case % 2 == 0 && !d.is_empty() {
            d[0] = d[0] & !0b110 | 0b010;
        }
        let limit = (next() % 2000) as usize;
        let _ = agree(&s, &d, limit, &format!("case {case}"));
    }
}

#[test]
fn unfiltering_is_the_same_on_both_paths() {
    // a small deterministic generator: every filter, 1 to 8 bytes a pixel,
    // widths that are not multiples of anything
    let mut x = 0x2545_f491_4f6c_dd1du64;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for bpp in 1..=8usize {
        for w in [1usize, 2, 3, 5, 16, 17, 33, 64, 127] {
            let stride = w * bpp;
            let h = 9;
            let mut raw = vec![0u8; (stride + 1) * h];
            for (k, b) in raw.iter_mut().enumerate() {
                *b = next() as u8;
                if k % (stride + 1) == 0 {
                    *b = (next() % 5) as u8;
                }
            }
            let rust = unfilter_rust(&raw, stride, h, bpp).unwrap();
            let used = unfilter(&raw, stride, h, bpp).unwrap();
            assert_eq!(used, rust, "bpp {bpp}, width {w}, native {NATIVE}");
        }
    }
    // the same errors
    let bad = [5u8, 1, 2, 3, 4];
    assert!(unfilter(&bad, 4, 1, 4).is_err() && unfilter_rust(&bad, 4, 1, 4).is_err());
    assert!(unfilter(&bad, 3, 1, 3).is_err() && unfilter_rust(&bad, 3, 1, 3).is_err());
}
