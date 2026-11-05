//! Where `kebitx migrate` spends its time, source by source, on the pack in
//! `KEBITX_PROFILE_PACK` (a Java pack folder, `.zip` or `.jar`). Not a test:
//! `cargo test --release -p kebitx-migrate --test profile -- --ignored --nocapture`.

use kebitx_core::vanilla::VanillaCatalog;
use kebitx_migrate::content::{BedrockPixels, JavaPixels};
use kebitx_migrate::lang::BedrockLang;
use kebitx_migrate::{input, inspect, JavaAssets, Sources};
use std::time::Instant;

#[test]
#[ignore]
fn stages() {
    let Some(pack) = std::env::var_os("KEBITX_PROFILE_PACK") else {
        return;
    };
    let t = Instant::now();
    let input = input::open(std::path::Path::new(&pack)).unwrap();
    eprintln!("  open (extract): {:?}", t.elapsed());
    let root = input.root.clone();

    let t = Instant::now();
    let java = JavaAssets::discover().unwrap();
    let jar = java.jar_for(None).unwrap();
    eprintln!("  Java game + jar: {:?}", t.elapsed());

    let t = Instant::now();
    let _ = java.sounds_json();
    eprintln!("  Java sounds.json: {:?}", t.elapsed());

    let t = Instant::now();
    let _jp = JavaPixels::from_jar(&jar);
    eprintln!("  JavaPixels (every texture of the jar): {:?}", t.elapsed());

    let install = kebitx_core::vanilla::install_dir().unwrap();
    let t = Instant::now();
    let bp = BedrockPixels::from_install(&install);
    eprintln!(
        "  BedrockPixels ({} textures): {:?}",
        bp.stems.len(),
        t.elapsed()
    );

    let t = Instant::now();
    let _ = BedrockLang::from_install(&install);
    eprintln!("  BedrockLang: {:?}", t.elapsed());

    let t = Instant::now();
    let catalog = VanillaCatalog::discover().unwrap_or_else(VanillaCatalog::empty);
    eprintln!("  catalog: {:?}", t.elapsed());

    let t = Instant::now();
    let sources = Sources::discover_for(Some(&root), None);
    eprintln!(
        "  Sources::discover_for (all of the above that the pack needs): {:?}",
        t.elapsed()
    );

    let t = Instant::now();
    let r = inspect(&root, sources.refs(&catalog));
    eprintln!(
        "  inspect ({} files): {:?}",
        r.images.len() + r.texts.len(),
        t.elapsed()
    );
}

/// The game's textures, phase by phase, on one core then on all of them:
/// reading the files, inflating, the rest of decoding, hashing.
#[test]
#[ignore]
fn bedrock_pixel_phases() {
    let Some(install) = kebitx_core::vanilla::install_dir() else {
        return;
    };
    let t = Instant::now();
    let files = kebitx_core::vanilla::files::map_files(
        &install,
        |p| p.starts_with("textures/") && (p.ends_with(".png") || p.ends_with(".tga")),
        |_, bytes| Some(bytes.to_vec()),
    );
    let read = t.elapsed();
    let files: Vec<(String, Vec<u8>)> = files.into_iter().collect();
    let total: usize = files.iter().map(|(_, b)| b.len()).sum();

    let t = Instant::now();
    let imgs: Vec<_> = files
        .iter()
        .filter_map(|(p, b)| kebitx_migrate::content::decode(p, b))
        .collect();
    let decode1 = t.elapsed();

    let t = Instant::now();
    let _ = kebitx_core::par::par_map(&files, |(p, b)| kebitx_migrate::content::decode(p, b));
    let decode_all = t.elapsed();

    let t = Instant::now();
    for i in &imgs {
        std::hint::black_box((
            kebitx_migrate::content::hash(i),
            kebitx_migrate::content::mask_hash(i),
        ));
    }
    let hash1 = t.elapsed();
    let px: usize = imgs.iter().map(|i| i.pixels.len()).sum();
    eprintln!(
        "{} textures, {:.1} MB in files, {:.1} MB of pixels\n  read (all cores): {read:?}\n  decode: 1 core {decode1:?}, all cores {decode_all:?}\n  hash + shape (1 core): {hash1:?}",
        files.len(),
        total as f64 / 1e6,
        px as f64 / 1e6
    );
}

/// Writes every 8-bit PNG of the game as its filtered rows (after inflating)
/// to `KEBITX_UNFILTER_CORPUS`: `count`, then per image `width`, `height`,
/// `bpp` (u32 little-endian) and `(stride + 1) * height` bytes. The same
/// input for unfiltering in Rust and in Zig (`native/zig/bench-unfilter`).
#[test]
#[ignore]
fn write_unfilter_corpus() {
    let (Some(out), Some(install)) = (
        std::env::var_os("KEBITX_UNFILTER_CORPUS"),
        kebitx_core::vanilla::install_dir(),
    ) else {
        return;
    };
    let files = kebitx_core::vanilla::files::map_files(
        &install,
        |p| p.starts_with("textures/") && p.ends_with(".png"),
        |_, bytes| Some(bytes.to_vec()),
    );
    let mut names: Vec<&String> = files.keys().collect();
    names.sort();
    let (mut body, mut n) = (Vec::new(), 0u32);
    for name in names {
        let b = &files[name];
        let (mut at, mut idat, mut ihdr) = (8usize, Vec::new(), None);
        while at + 8 <= b.len() {
            let len = u32::from_be_bytes(b[at..at + 4].try_into().unwrap()) as usize;
            match &b[at + 4..at + 8] {
                b"IHDR" => ihdr = Some(b[at + 8..at + 8 + 13].to_vec()),
                b"IDAT" => idat.extend_from_slice(&b[at + 8..at + 8 + len]),
                _ => {}
            }
            at += 12 + len;
        }
        let Some(h) = ihdr else { continue };
        let (w, ht) = (
            u32::from_be_bytes(h[0..4].try_into().unwrap()),
            u32::from_be_bytes(h[4..8].try_into().unwrap()),
        );
        let (depth, color, interlace) = (h[8], h[9], h[12]);
        let channels = match color {
            0 | 3 => 1,
            2 => 3,
            4 => 2,
            _ => 4,
        };
        if depth != 8 || interlace != 0 {
            continue;
        }
        let stride = w as usize * channels;
        let Ok(raw) = kebitx_core::codec::zlib_decompress(&idat, (stride + 1) * ht as usize) else {
            continue;
        };
        if raw.len() != (stride + 1) * ht as usize {
            continue;
        }
        for v in [w, ht, channels as u32] {
            body.extend(v.to_le_bytes());
        }
        body.extend(&raw);
        n += 1;
    }
    let mut buf = n.to_le_bytes().to_vec();
    buf.extend(body);
    std::fs::write(&out, &buf).unwrap();
    eprintln!(
        "{n} images, {:.1} MB of filtered rows",
        (buf.len() - 4) as f64 / 1e6
    );
}

/// Unfiltering the corpus of `write_unfilter_corpus` in Rust: time, and a
/// checksum of every output byte to compare with other implementations.
#[test]
#[ignore]
fn unfilter_corpus() {
    let Some(path) = std::env::var_os("KEBITX_UNFILTER_CORPUS") else {
        return;
    };
    let buf = std::fs::read(path).unwrap();
    let le = |i: usize| u32::from_le_bytes(buf[i..i + 4].try_into().unwrap()) as usize;
    let n = le(0);
    let mut images = Vec::new();
    let mut at = 4;
    for _ in 0..n {
        let (w, h, bpp) = (le(at), le(at + 4), le(at + 8));
        let len = (w * bpp + 1) * h;
        images.push((w * bpp, h, bpp, &buf[at + 12..at + 12 + len]));
        at += 12 + len;
    }
    let mut best = std::time::Duration::MAX;
    let mut outs = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        outs = images
            .iter()
            .map(|(stride, h, bpp, raw)| {
                kebitx_core::codec::png::unfilter(raw, *stride, *h, *bpp).unwrap()
            })
            .collect::<Vec<_>>();
        best = best.min(t.elapsed());
    }
    // the checksum, outside the timing: the sum of each image's CRC-32
    let sum = outs.iter().fold(0u64, |s, r| {
        s.wrapping_add(kebitx_core::codec::crc32(r) as u64)
    });
    let bytes: usize = images.iter().map(|(s, h, ..)| s * h).sum();
    eprintln!(
        "Rust unfilter: {n} images, {:.1} MB, best of 5: {best:?} ({:.0} MB/s), checksum {sum:x}",
        bytes as f64 / 1e6,
        bytes as f64 / 1e6 / best.as_secs_f64()
    );
}

/// `map_files` with more and more of `BedrockPixels`'s work per texture.
#[test]
#[ignore]
fn bedrock_pixels_steps() {
    use kebitx_migrate::content::{decode, hash, mask_hash};
    let Some(install) = kebitx_core::vanilla::install_dir() else {
        return;
    };
    let wanted =
        |p: &str| p.starts_with("textures/") && (p.ends_with(".png") || p.ends_with(".tga"));
    let step = |label: &str, f: &(dyn Fn(&str, &[u8]) -> Option<u64> + Sync)| {
        let t = Instant::now();
        let n = kebitx_core::vanilla::files::map_files(&install, wanted, |p, b| f(p, b)).len();
        eprintln!("  {label}: {:?} ({n})", t.elapsed());
    };
    step("read only", &|_, b| Some(b.len() as u64));
    step("+ decode", &|p, b| decode(p, b).map(|i| i.width as u64));
    step("+ hash", &|p, b| decode(p, b).map(|i| hash(&i)));
    step("+ shape", &|p, b| {
        decode(p, b).map(|i| hash(&i) ^ mask_hash(&i).unwrap_or(0))
    });
}

/// The game's PNGs by colour type and depth, and the time of each part of
/// decoding them on one core: zlib, unfiltering, and the rest (chunks and
/// the conversion to RGBA).
#[test]
#[ignore]
fn png_parts() {
    let Some(install) = kebitx_core::vanilla::install_dir() else {
        return;
    };
    let files = kebitx_core::vanilla::files::map_files(
        &install,
        |p| p.starts_with("textures/") && p.ends_with(".png"),
        |_, b| Some(b.to_vec()),
    );
    let mut kinds: std::collections::BTreeMap<(u8, u8), (usize, usize)> = Default::default();
    let mut parts = Vec::new();
    for b in files.values() {
        let (mut at, mut idat, mut ihdr) = (8usize, Vec::new(), None);
        while at + 8 <= b.len() {
            let len = u32::from_be_bytes(b[at..at + 4].try_into().unwrap()) as usize;
            match &b[at + 4..at + 8] {
                b"IHDR" => ihdr = Some(b[at + 8..at + 8 + 13].to_vec()),
                b"IDAT" => idat.extend_from_slice(&b[at + 8..at + 8 + len]),
                _ => {}
            }
            at += 12 + len;
        }
        let Some(h) = ihdr else { continue };
        let (w, ht) = (
            u32::from_be_bytes(h[0..4].try_into().unwrap()) as usize,
            u32::from_be_bytes(h[4..8].try_into().unwrap()) as usize,
        );
        let (depth, color) = (h[8], h[9]);
        if h[12] != 0 {
            // interlaced: its parts are not the rows measured here
            continue;
        }
        let k = kinds.entry((color, depth)).or_default();
        k.0 += 1;
        k.1 += w * ht;
        let ch = match color {
            0 | 3 => 1,
            2 => 3,
            4 => 2,
            _ => 4,
        };
        let bits = ch * depth as usize;
        let stride = (w * bits).div_ceil(8);
        parts.push((b.clone(), idat, stride, ht, bits.div_ceil(8).max(1)));
    }
    for ((c, d), (n, px)) in &kinds {
        eprintln!(
            "  colour {c} depth {d}: {n} images, {:.1} M pixels",
            *px as f64 / 1e6
        );
    }
    let t = Instant::now();
    let raws: Vec<_> = parts
        .iter()
        .map(|(_, idat, stride, h, _)| {
            kebitx_core::codec::zlib_decompress(idat, (stride + 1) * h).unwrap()
        })
        .collect();
    let zlib = t.elapsed();
    let t = Instant::now();
    for ((_, _, stride, h, bpp), raw) in parts.iter().zip(&raws) {
        std::hint::black_box(kebitx_core::codec::png::unfilter(raw, *stride, *h, *bpp).unwrap());
    }
    let unfilter = t.elapsed();
    let t = Instant::now();
    for (b, ..) in &parts {
        std::hint::black_box(kebitx_core::codec::png::decode(b).unwrap());
    }
    let whole = t.elapsed();
    eprintln!(
        "  1 core: whole decode {whole:?} = zlib {zlib:?} + unfilter {unfilter:?} + the rest {:?} (native: {})",
        whole.saturating_sub(zlib + unfilter),
        kebitx_core::codec::NATIVE
    );
}

/// Which of the game's PNGs kebitx refuses, and why: the chunk whose CRC
/// does not match, and whether it is critical (upper-case first letter).
#[test]
#[ignore]
fn refused_pngs() {
    let Some(install) = kebitx_core::vanilla::install_dir() else {
        return;
    };
    let files = kebitx_core::vanilla::files::map_files(
        &install,
        |p| p.starts_with("textures/") && p.ends_with(".png"),
        |_, b| Some(b.to_vec()),
    );
    let mut names: Vec<&String> = files.keys().collect();
    names.sort();
    for n in names {
        let b = &files[n];
        let Err(e) = kebitx_core::codec::png::decode(b) else {
            continue;
        };
        let mut bad = Vec::new();
        let mut at = 8;
        while at + 12 <= b.len() {
            let len = u32::from_be_bytes(b[at..at + 4].try_into().unwrap()) as usize;
            if at + 12 + len > b.len() {
                break;
            }
            let kind = String::from_utf8_lossy(&b[at + 4..at + 8]).to_string();
            let crc = u32::from_be_bytes(b[at + 8 + len..at + 12 + len].try_into().unwrap());
            if kebitx_core::codec::crc32(&b[at + 4..at + 8 + len]) != crc {
                bad.push(kind);
            }
            at += 12 + len;
        }
        eprintln!("  {n}: {e}; chunks with a wrong CRC: {bad:?}");
    }
}

/// Writes PNGs of the game to `KEBITX_DUMP` (every interlaced one, every one
/// with a damaged ancillary chunk, and every 25th other), and `kebitx.txt`:
/// each file and the CRC-32 of the RGBA pixels kebitx decodes, to compare
/// with an independent decoder.
#[test]
#[ignore]
fn dump_pngs_for_comparison() {
    let (Some(dir), Some(install)) = (
        std::env::var_os("KEBITX_DUMP"),
        kebitx_core::vanilla::install_dir(),
    ) else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let files = kebitx_core::vanilla::files::map_files(
        &install,
        |p| p.starts_with("textures/") && p.ends_with(".png"),
        |_, b| Some(b.to_vec()),
    );
    let mut names: Vec<&String> = files.keys().collect();
    names.sort();
    let mut sums = String::new();
    for (k, n) in names.iter().enumerate() {
        let b = &files[*n];
        let interlaced = b.len() > 28 && b[28] == 1;
        let damaged = b.windows(4).any(|w| w == b"iCCP");
        if !(interlaced || damaged || k % 25 == 0) {
            continue;
        }
        let Ok(img) = kebitx_core::codec::png::decode(b) else {
            continue;
        };
        let file = n.replace('/', "__");
        std::fs::write(dir.join(&file), b).unwrap();
        sums.push_str(&format!(
            "{file} {:08x}\n",
            kebitx_core::codec::crc32(&img.pixels)
        ));
    }
    std::fs::write(dir.join("kebitx.txt"), &sums).unwrap();
    eprintln!("{} images written", sums.lines().count());
}
