//! Mutation fuzzing on stable Rust, std only.
//!
//! Every reader of untrusted input gets thousands of damaged versions of real
//! files: bits flipped, bytes replaced by the characters parsers care about,
//! ranges deleted or duplicated, truncations, random insertions. Properties:
//!
//! 1. no reader panics, whatever the input;
//! 2. whatever JSON parses, written back and parsed again, is the same value.
//!
//! Deterministic (a fixed-seed xorshift), so a failure always reproduces.
//! `KEBITX_FUZZ_ITERS=<n>` runs n mutations per seed (default 300) for longer
//! sessions. Complements, does not replace, coverage-guided fuzzing.

mod common;
use common::fixture;
use kebitx_core::image;
use kebitx_core::json::{self, Json};
use kebitx_core::manifest::parse_manifest;
use kebitx_core::nbt::parse_level_dat;
use kebitx_core::parser::atlas::parse_atlas;
use kebitx_core::parser::material::material_names;
use kebitx_core::parser::model::parse_models;
use kebitx_core::parser::parse_client_entity;
use kebitx_core::parser::render_controller::parse_render_controller;
use kebitx_core::vanilla::brarchive::{Brarchive, MAGIC};
use std::fs;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

/// Bytes that change how parsers branch.
const INTERESTING: &[u8] = b"{}[]\":,\\-0919eE.\0\xff\x80\n /*";

fn mutate(seed: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut b = seed.to_vec();
    for _ in 0..1 + rng.below(4) {
        let len = b.len();
        match rng.below(6) {
            0 if len > 0 => {
                let i = rng.below(len);
                b[i] ^= 1 << rng.below(8);
            }
            1 if len > 0 => {
                let i = rng.below(len);
                b[i] = INTERESTING[rng.below(INTERESTING.len())];
            }
            2 if len > 0 => {
                let a = rng.below(len);
                let e = (a + 1 + rng.below(64)).min(len);
                b.drain(a..e);
            }
            3 if len > 0 => {
                let a = rng.below(len);
                let e = (a + 1 + rng.below(64)).min(len);
                let chunk = b[a..e].to_vec();
                let at = rng.below(b.len());
                b.splice(at..at, chunk);
            }
            4 => b.truncate(rng.below(len + 1)),
            _ => {
                let at = rng.below(len + 1);
                let n = 1 + rng.below(16);
                let junk: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
                b.splice(at..at, junk);
            }
        }
    }
    b
}

fn iterations() -> usize {
    std::env::var("KEBITX_FUZZ_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300)
}

/// Every text reader at once, on one input; and the JSON round trip.
fn text_readers(bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    let _ = parse_client_entity(&text);
    let _ = parse_atlas(&text);
    if let Ok(v) = json::parse(&text) {
        let _ = parse_models(&v);
        let _ = parse_manifest(&v);
        let _ = material_names(&v);
        let _ = parse_render_controller(&v);
        if let Some(rcs) = v.get("render_controllers").and_then(Json::as_object) {
            for body in rcs.values() {
                let _ = parse_render_controller(body);
            }
        }
        let written = json::to_string(&v);
        let again = json::parse(&written)
            .unwrap_or_else(|e| panic!("written JSON does not parse back ({}): {written}", e.msg));
        assert_eq!(again, v, "round trip changed the value");
    }
}

/// Small real files of every kind the readers see.
fn text_seeds() -> Vec<Vec<u8>> {
    let mut seeds = Vec::new();
    for f in kebitx_core::filesystem::all_files(&fixture("")) {
        let name = f.to_string_lossy().to_ascii_lowercase();
        let small = fs::metadata(&f)
            .map(|m| m.len() < 16 * 1024)
            .unwrap_or(false);
        if small && (name.ends_with(".json") || name.ends_with(".material")) {
            seeds.push(fs::read(&f).unwrap());
        }
    }
    seeds.sort();
    seeds.dedup();
    assert!(
        seeds.len() > 50,
        "the fixtures should give many seeds, got {}",
        seeds.len()
    );
    seeds
}

#[test]
fn text_readers_never_panic_and_json_round_trips() {
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let seeds = text_seeds();
    for seed in &seeds {
        text_readers(seed);
        for _ in 0..iterations() {
            text_readers(&mutate(seed, &mut rng));
        }
    }
}

#[test]
fn binary_readers_never_panic() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    // a small valid bundle
    let mut bundle = MAGIC.to_vec();
    bundle.extend(2u32.to_le_bytes());
    bundle.extend([0u8; 4]);
    for (name, off, size) in [("a.json", 0u32, 2u32), ("b.png", 2, 3)] {
        let mut e = [0u8; 256];
        e[0] = name.len() as u8;
        e[1..1 + name.len()].copy_from_slice(name.as_bytes());
        e[248..252].copy_from_slice(&off.to_le_bytes());
        e[252..256].copy_from_slice(&size.to_le_bytes());
        bundle.extend(e);
    }
    bundle.extend(b"{}xyz");
    let level_dat = fs::read(fixture("world_scripts/level.dat")).unwrap();
    let mut images = Vec::new();
    for f in kebitx_core::filesystem::all_files(&fixture("")) {
        let n = f.to_string_lossy().to_ascii_lowercase();
        if (n.ends_with(".png") || n.ends_with(".tga"))
            && fs::metadata(&f).is_ok_and(|m| m.len() > 0 && m.len() < 4096)
        {
            images.push(fs::read(&f).unwrap());
        }
    }
    assert!(!images.is_empty());

    for _ in 0..iterations() * 20 {
        let b = mutate(&bundle, &mut rng);
        if let Ok(a) = Brarchive::from_bytes(b) {
            let _ = a.is_consistent();
            let _ = a.read_matching(|_| true);
        }
        let _ = parse_level_dat(&mutate(&level_dat, &mut rng));
        let img = mutate(&images[rng.below(images.len())], &mut rng);
        let _ = image::png(&img);
        let _ = image::jpeg(&img);
        let _ = image::tga(&img);
    }
}
