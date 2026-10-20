//! Where the time goes, stage by stage, on the installed game and on mods
//! given in `KEBITX_PROFILE_MODS` (a mods folder). Not a test: run with
//! `cargo test --release -p kebitx-java --test profile -- --ignored --nocapture`.

use kebitx_java::code::GameIndex;
use kebitx_java::{JavaAssets, Source};
use std::time::Instant;

#[test]
#[ignore]
fn stages() {
    let Some(java) = JavaAssets::discover() else {
        return;
    };
    let Some(jar) = java.jar_for(None) else {
        return;
    };
    let names: Vec<String> = jar
        .zip
        .names()
        .filter(|n| n.ends_with(".class"))
        .cloned()
        .collect();

    let t = Instant::now();
    let mut bytes = Vec::with_capacity(names.len());
    for n in &names {
        bytes.push(jar.zip.read(n, 16 << 20).unwrap());
    }
    let read = t.elapsed();
    let total: usize = bytes.iter().map(Vec::len).sum();

    // the same entries from the archive held in memory: no file opened per entry
    let path = java
        .dir
        .join("versions")
        .join(&jar.id)
        .join(format!("{}.jar", jar.id));
    let t = Instant::now();
    let mem =
        kebitx_core::codec::zip::ZipArchive::from_bytes(std::fs::read(&path).unwrap()).unwrap();
    let loaded = t.elapsed();
    let t = Instant::now();
    for n in &names {
        mem.read(n, 16 << 20).unwrap();
    }
    let inflate = t.elapsed();
    eprintln!(
        "  archive into memory: {loaded:?}; inflate only (1 core): {inflate:?} ({:.0} MB/s)",
        total as f64 / 1e6 / inflate.as_secs_f64()
    );

    let t = Instant::now();
    let mut parsed = 0;
    for b in &bytes {
        parsed += kebitx_java::class::parse(b).is_ok() as usize;
    }
    let parse = t.elapsed();

    let t = Instant::now();
    let game = GameIndex::from_jar(&jar);
    let index = t.elapsed();

    eprintln!(
        "{}: {} classes, {:.1} MB inflated\n  read+inflate (1 core): {read:?} ({:.0} MB/s)\n  parse (1 core): {parse:?}\n  GameIndex::from_jar (all cores): {index:?}",
        jar.id,
        parsed,
        total as f64 / 1e6,
        total as f64 / 1e6 / read.as_secs_f64()
    );

    if let Some(dir) = std::env::var_os("KEBITX_PROFILE_MODS") {
        let dir = std::path::PathBuf::from(dir);
        let t = Instant::now();
        let r = kebitx_java::modset::analyze_set(&dir, Some(&jar));
        eprintln!("  analyze_set ({} mods): {:?}", r.mods.len(), t.elapsed());
        for p in kebitx_java::modset::jars_in(&dir) {
            let t = Instant::now();
            let src = Source::open(&p).unwrap();
            let rep = kebitx_java::analyze::analyze_in(
                &src,
                Some(&jar),
                Some(&game),
                &Default::default(),
            );
            eprintln!(
                "  analyze_in {} (index shared): {:?} ({} classes)",
                p.file_name().unwrap().to_string_lossy(),
                t.elapsed(),
                rep.code.map_or(0, |c| c.classes)
            );
        }
    }
}

/// Writes every class entry of the game's `.jar` as stored (raw DEFLATE) to
/// `KEBITX_CORPUS`: `count`, then per entry `packed_len`, `size`, `crc32`
/// (u32 little-endian) and the bytes. The same input for every decoder
/// measured (`native/zig/bench-inflate`).
#[test]
#[ignore]
fn write_inflate_corpus() {
    let (Some(out), Some(java)) = (std::env::var_os("KEBITX_CORPUS"), JavaAssets::discover())
    else {
        return;
    };
    let jar = java.jar_for(None).unwrap();
    let mut names: Vec<&String> = jar.zip.names().filter(|n| n.ends_with(".class")).collect();
    names.sort();
    let mut buf = Vec::new();
    let mut n = 0u32;
    let mut body = Vec::new();
    for name in names {
        let (method, size, crc, packed) = jar.zip.read_raw(name, 16 << 20).unwrap();
        if method != 8 {
            continue;
        }
        body.extend((packed.len() as u32).to_le_bytes());
        body.extend((size as u32).to_le_bytes());
        body.extend(crc.to_le_bytes());
        body.extend(&packed);
        n += 1;
    }
    buf.extend(n.to_le_bytes());
    buf.extend(body);
    std::fs::write(&out, &buf).unwrap();

    // and the Rust decoder on exactly that input
    let t = Instant::now();
    let (mut at, mut total) = (4usize, 0usize);
    for _ in 0..n {
        let le = |i: usize| u32::from_le_bytes(buf[i..i + 4].try_into().unwrap()) as usize;
        let (plen, size, crc) = (le(at), le(at + 4), le(at + 8) as u32);
        let data = kebitx_core::codec::inflate::inflate(&buf[at + 12..at + 12 + plen], size)
            .unwrap()
            .0;
        assert_eq!(kebitx_core::codec::crc32(&data), crc);
        total += data.len();
        at += 12 + plen;
    }
    let el = t.elapsed();
    // the CRC alone, on the same output
    let outs: Vec<Vec<u8>> = {
        let (mut at, mut v) = (4usize, Vec::new());
        for _ in 0..n {
            let le = |i: usize| u32::from_le_bytes(buf[i..i + 4].try_into().unwrap()) as usize;
            let (plen, size) = (le(at), le(at + 4));
            v.push(
                kebitx_core::codec::inflate::inflate(&buf[at + 12..at + 12 + plen], size)
                    .unwrap()
                    .0,
            );
            at += 12 + plen;
        }
        v
    };
    let t = Instant::now();
    let mut x = 0u32;
    for o in &outs {
        x ^= kebitx_core::codec::crc32(o);
    }
    let crc_time = t.elapsed();
    eprintln!(
        "  of which CRC-32 alone: {crc_time:?} ({:.0} MB/s) [{x:x}]",
        total as f64 / 1e6 / crc_time.as_secs_f64()
    );
    eprintln!(
        "corpus: {n} streams, {:.1} MB packed, {:.1} MB out; Rust inflate + CRC: {el:?} ({:.0} MB/s)",
        (buf.len() - 4) as f64 / 1e6,
        total as f64 / 1e6,
        total as f64 / 1e6 / el.as_secs_f64()
    );
}

/// `GameIndex::from_jar`, phase by phase: what a faster decoder could win.
#[test]
#[ignore]
fn index_phases() {
    let Some(java) = JavaAssets::discover() else {
        return;
    };
    let jar = java.jar_for(None).unwrap();
    let names: Vec<&String> = jar.zip.names().filter(|n| n.ends_with(".class")).collect();

    let t = Instant::now();
    let raw: Vec<(u64, Vec<u8>)> = names
        .iter()
        .map(|n| {
            let (_, size, _, b) = jar.zip.read_raw(n, 16 << 20).unwrap();
            (size, b)
        })
        .collect();
    let io = t.elapsed();

    let t = Instant::now();
    let inflated = kebitx_core::par::par_map(&raw, |(size, b)| {
        kebitx_core::codec::inflate::inflate(b, *size as usize)
            .unwrap()
            .0
    });
    let inflate = t.elapsed();

    let t = Instant::now();
    let parsed = kebitx_core::par::par_map(&inflated, |b| kebitx_java::class::parse(b).unwrap());
    let parse = t.elapsed();

    let t = Instant::now();
    let mut g = GameIndex::default();
    for c in &parsed {
        g.add(c);
    }
    let add = t.elapsed();

    let t = Instant::now();
    let _ = GameIndex::from_jar(&jar);
    let whole = t.elapsed();
    eprintln!(
        "index of {} classes on {} threads: read (serial) {io:?}, inflate (parallel) {inflate:?}, parse (parallel) {parse:?}, add (serial) {add:?}; from_jar as is: {whole:?}",
        names.len(),
        kebitx_core::par::threads()
    );
}
