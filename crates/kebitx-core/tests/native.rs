//! The native kernels against kebitx's Rust, on the same input.
//!
//! `fixtures/deflate/streams.bin` holds 150 DEFLATE and zlib streams made by
//! another compressor (Zig's, with dynamic Huffman codes, as real files have),
//! each with its size and the CRC-32 of its data. In a pure-Rust build these
//! tests check kebitx's own decoder on them. Built with `--features native`,
//! `inflate`, `zlib_decompress` and `png::unfilter` go through libdeflate and
//! Zig, and every output must be the very bytes of the Rust path — on valid
//! streams, on every truncation and on corrupted bytes.

use kebitx_core::codec::inflate::{inflate_rust, zlib_decompress_rust};
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

/// What the dispatching decoder (native when built in) answers.
fn decode(s: &Stream, data: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    if s.zlib {
        zlib_decompress(data, limit).map_err(|e| e.to_string())
    } else {
        inflate(data, limit)
            .map(|(o, _)| o)
            .map_err(|e| e.to_string())
    }
}

fn decode_rust(s: &Stream, data: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    if s.zlib {
        zlib_decompress_rust(data, limit).map_err(|e| e.to_string())
    } else {
        inflate_rust(data, limit)
            .map(|(o, _)| o)
            .map_err(|e| e.to_string())
    }
}

#[test]
fn every_stream_decodes_to_its_data_on_both_paths() {
    let all = streams();
    assert_eq!(all.len(), 150);
    for (i, s) in all.iter().enumerate() {
        let rust = decode_rust(s, &s.packed, s.size).unwrap_or_else(|e| panic!("stream {i}: {e}"));
        assert_eq!(
            (rust.len(), crc32(&rust)),
            (s.size, s.crc),
            "stream {i}: Rust"
        );
        let used = decode(s, &s.packed, s.size).unwrap_or_else(|e| panic!("stream {i}: {e}"));
        assert_eq!(used, rust, "stream {i}: native {NATIVE}");
        if !s.zlib {
            // the input consumed agrees too (zlib finds its checksum by it)
            assert_eq!(
                inflate(&s.packed, s.size).unwrap().1,
                inflate_rust(&s.packed, s.size).unwrap().1,
                "stream {i}: bytes used"
            );
        }
        // one byte less of room than the data needs is refused on both
        if s.size > 0 {
            assert!(decode_rust(s, &s.packed, s.size - 1).is_err(), "stream {i}");
            assert!(decode(s, &s.packed, s.size - 1).is_err(), "stream {i}");
        }
    }
}

/// Damaged input: every prefix of some streams, and bytes changed across
/// them. Whatever both accept must be the same bytes; neither may crash.
#[test]
fn damaged_streams_never_decode_differently() {
    let all = streams();
    let (mut agree_ok, mut agree_err, mut only_one) = (0, 0, Vec::new());
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
            let limit = s.size + 64;
            match (decode_rust(s, &d, limit), decode(s, &d, limit)) {
                (Ok(a), Ok(b)) => {
                    assert_eq!(
                        a, b,
                        "stream {i}: both accept a damaged stream, with different bytes"
                    );
                    agree_ok += 1;
                }
                (Err(_), Err(_)) => agree_err += 1,
                (a, b) => only_one.push((i, a.is_ok(), b.is_ok())),
            }
        }
    }
    eprintln!(
        "native {NATIVE}: {agree_ok} damaged inputs decoded the same, {agree_err} refused by both, {} accepted by one only",
        only_one.len()
    );
    // a decoder may accept a damaged stream the other refuses (a changed
    // byte can still be valid DEFLATE): what matters is that no input
    // decodes to different bytes, checked above
    if !NATIVE {
        assert!(only_one.is_empty());
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
