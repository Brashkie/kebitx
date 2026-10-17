//! Optimization, with phones as the reference target.
//!
//! Everything here is INFORMATION: a big texture or a heavy model works; it
//! costs memory, space or frame time on a weak device. Only what can be
//! measured from the files is reported, with the measurement:
//!
//! - pack icons larger than the game ever shows them;
//! - textures whose decoded size alone is a large share of a phone's video
//!   memory (width × height × 4 bytes, before any mipmaps);
//! - images whose content is another format than their extension says (a
//!   JPEG named `.png` has no transparency, and tools that trust the name
//!   misread it);
//! - models with many cubes (each cube is up to 12 triangles, every frame).
//!
//! Not reported, on purpose: textures whose sides are not powers of two.
//! Modern mobile GPUs handle them; calling them a problem would be a guess.

use crate::filesystem::{all_files, discover_models};
use crate::image::{probe, Format};
use crate::json;
use crate::manifest::{Pack, PackKind};
use crate::par::par_map;
use crate::parser::model::parse_models;
use std::fs;
use std::path::PathBuf;

/// The game shows pack icons small; 256 px a side is already generous.
pub const ICON_MAX_SIDE: u32 = 256;
pub const ICON_MAX_BYTES: u64 = 200 * 1024;
/// 2048 × 2048 × 4 bytes = 16 MiB of video memory for one texture.
pub const BIG_TEXTURE_PIXELS: u64 = 2048 * 2048;
pub const HEAVY_MODEL_CUBES: usize = 500;

#[derive(Debug, Clone, PartialEq)]
pub enum OptIssue {
    BigIcon {
        pack: String,
        width: u32,
        height: u32,
        bytes: u64,
    },
    BigTexture {
        pack: String,
        path: String,
        width: u32,
        height: u32,
        video_bytes: u64,
    },
    MislabelledImage {
        pack: String,
        path: String,
        content: Format,
    },
    HeavyModel {
        geometry: String,
        file: String,
        cubes: usize,
        bones: usize,
    },
}

#[derive(Debug, Default)]
pub struct OptReport {
    pub images: usize,
    /// Images whose size could not be read (unknown or damaged format).
    pub unreadable_images: usize,
    /// Decoded size of every readable texture, summed: what the packs could
    /// ask of video memory if everything were loaded at once.
    pub video_bytes: u64,
    pub models: usize,
    pub cubes: usize,
    pub issues: Vec<OptIssue>,
}

pub fn run(packs: &[Pack]) -> OptReport {
    let mut report = OptReport::default();

    // ---- pack icons, every pack
    for p in packs {
        let icon = p.root.join("pack_icon.png");
        let Some(info) = probe(&icon) else { continue };
        let bytes = fs::metadata(&icon).map(|m| m.len()).unwrap_or(0);
        if info.width > ICON_MAX_SIDE || info.height > ICON_MAX_SIDE || bytes > ICON_MAX_BYTES {
            report.issues.push(OptIssue::BigIcon {
                pack: p.folder.clone(),
                width: info.width,
                height: info.height,
                bytes,
            });
        }
    }

    let rps: Vec<&Pack> = packs
        .iter()
        .filter(|p| p.manifest.kind == PackKind::Resources)
        .collect();

    // ---- textures, read from their headers on every core
    let images: Vec<(&Pack, PathBuf)> = rps
        .iter()
        .flat_map(|p| {
            all_files(&p.root.join("textures"))
                .into_iter()
                .filter(|f| {
                    let n = f.to_string_lossy().to_ascii_lowercase();
                    [".png", ".tga", ".jpg", ".jpeg"]
                        .iter()
                        .any(|e| n.ends_with(e))
                })
                .map(move |f| (*p, f))
        })
        .collect();
    report.images = images.len();
    let probed = par_map(&images, |(_, f)| probe(f));
    for ((p, f), info) in images.iter().zip(probed) {
        let rel = f
            .strip_prefix(&p.root)
            .unwrap_or(f)
            .to_string_lossy()
            .replace('\\', "/");
        let Some(info) = info else {
            report.unreadable_images += 1;
            continue;
        };
        let pixels = info.width as u64 * info.height as u64;
        report.video_bytes += pixels * 4;
        if pixels >= BIG_TEXTURE_PIXELS {
            report.issues.push(OptIssue::BigTexture {
                pack: p.folder.clone(),
                path: rel.clone(),
                width: info.width,
                height: info.height,
                video_bytes: pixels * 4,
            });
        }
        let lower = rel.to_ascii_lowercase();
        let named = if lower.ends_with(".png") {
            Some(Format::Png)
        } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
            Some(Format::Jpeg)
        } else {
            None
        };
        if named.is_some_and(|n| n != info.format) {
            report.issues.push(OptIssue::MislabelledImage {
                pack: p.folder.clone(),
                path: rel,
                content: info.format,
            });
        }
    }

    // ---- models
    let files: Vec<(&Pack, PathBuf)> = rps
        .iter()
        .flat_map(|p| discover_models(&p.root).into_iter().map(move |f| (*p, f)))
        .collect();
    let parsed = par_map(&files, |(p, f)| {
        let rel = f
            .strip_prefix(&p.root)
            .unwrap_or(f)
            .to_string_lossy()
            .replace('\\', "/");
        let defs = crate::snapshot::read_to_string(f)
            .ok()
            .and_then(|s| json::parse(&s).ok())
            .map(|r| parse_models(&r))
            .unwrap_or_default();
        (format!("{}/{rel}", p.folder), defs)
    });
    for (file, defs) in parsed {
        for m in defs {
            let cubes: usize = m.bones.iter().map(|b| b.cubes.len()).sum();
            report.models += 1;
            report.cubes += cubes;
            if cubes >= HEAVY_MODEL_CUBES {
                report.issues.push(OptIssue::HeavyModel {
                    geometry: m.identifier.clone(),
                    file: file.clone(),
                    cubes,
                    bones: m.bones.len(),
                });
            }
        }
    }

    // the costliest first
    report.issues.sort_by_key(|i| std::cmp::Reverse(weight(i)));
    report
}

/// For ordering: bytes for textures and icons, cubes × 1 KiB for models.
fn weight(i: &OptIssue) -> u64 {
    match i {
        OptIssue::BigIcon { bytes, .. } => *bytes,
        OptIssue::BigTexture { video_bytes, .. } => *video_bytes,
        OptIssue::MislabelledImage { .. } => 0,
        OptIssue::HeavyModel { cubes, .. } => *cubes as u64 * 1024,
    }
}
