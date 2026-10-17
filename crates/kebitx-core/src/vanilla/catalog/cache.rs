//! The game catalog, kept on disk between runs.
//!
//! Building the catalog reads 1100+ bundles of the installed game; the game
//! does not change between two runs of kebitx, so the result is stored and
//! reused. A stale catalog would give wrong answers, so the cache is only used
//! when its KEY matches exactly:
//!
//! - the cache format,
//! - the install folder (it carries the game version: an update is a new path),
//! - the game version,
//! - the modification time of the game's `data/resource_packs`.
//!
//! The file is untrusted like any other input: every count is checked against
//! the bytes left before anything is allocated, and any problem — wrong magic,
//! other key, truncation, garbage — means "no cache": the catalog is rebuilt,
//! never half-loaded. It is written to a temporary file and renamed into place,
//! so a crash mid-write leaves the old cache or none, not a broken one.

use super::{TileRef, VanillaCatalog};
use crate::parser::render_controller::{RenderControllerDef, Slot, SlotUse};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const MAGIC: &[u8; 8] = b"KBXCATLG";
/// Bump whenever the catalog's content or this encoding changes.
const FORMAT: u32 = 5;
/// A real cache is about 1 MB; anything near this is not one.
const CACHE_MAX_BYTES: u64 = 256 * 1024 * 1024;

/// Where cache files live: `KEBITX_CACHE_DIR`, else the system's cache folder
/// (`%LOCALAPPDATA%`, `~/Library/Caches`, `$XDG_CACHE_HOME` or `~/.cache`)
/// under `kebitx/cache`, else the temp folder.
pub fn dir() -> PathBuf {
    if let Some(d) = std::env::var_os("KEBITX_CACHE_DIR") {
        return PathBuf::from(d);
    }
    let var = |k: &str| std::env::var_os(k).map(PathBuf::from);
    // each system's own place for caches; the temp folder only as a last
    // resort (systems clean it on their own)
    let base = if cfg!(windows) {
        var("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library").join("Caches"))
    } else {
        var("XDG_CACHE_HOME").or_else(|| var("HOME").map(|h| h.join(".cache")))
    };
    match base {
        Some(b) => b.join("kebitx").join("cache"),
        None => std::env::temp_dir().join("kebitx-cache"),
    }
}

fn file_for(install: &Path) -> PathBuf {
    dir().join(format!(
        "catalog-{:016x}.bin",
        fnv1a(install.to_string_lossy().as_bytes())
    ))
}

/// FNV-1a: a stable hash (std's `DefaultHasher` may change between Rust
/// versions, and a file name must not).
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)
    })
}

/// What a cache must match to be used.
fn key(install: &Path) -> String {
    let stack = super::super::stack_dir(install, "resource_packs")
        .or_else(|| super::super::samples_layer(install, "resource_packs"))
        .unwrap_or_else(|| install.to_path_buf());
    let mtime = fs::metadata(stack)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().to_string())
        .unwrap_or_else(|| "unknown".into());
    format!(
        "{FORMAT}|{}|{}|{mtime}",
        install.display(),
        super::super::game_version(install)
    )
}

pub fn load(install: &Path) -> Option<VanillaCatalog> {
    decode(
        &crate::filesystem::read_limited(&file_for(install), CACHE_MAX_BYTES).ok()?,
        &key(install),
    )
}

pub fn store(install: &Path, c: &VanillaCatalog) -> std::io::Result<()> {
    let file = file_for(install);
    fs::create_dir_all(dir())?;
    let tmp = file.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&tmp, encode(c, &key(install)))?;
    fs::rename(&tmp, &file).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

// ------------------------------------------------------------------ encoding

pub(super) fn encode(c: &VanillaCatalog, key: &str) -> Vec<u8> {
    let mut w = Writer(Vec::new());
    w.0.extend(MAGIC);
    w.u32(FORMAT);
    w.str(key);
    w.str(&c.version);
    w.u32(c.bundles_read as u32);
    w.u32(c.bundles_bad as u32);
    for set in [
        &c.textures,
        &c.geometries,
        &c.animations,
        &c.animation_controllers,
        &c.render_controllers,
        &c.asset_stems,
        &c.materials,
        &c.item_atlas,
    ] {
        w.set(set);
    }
    let mut sized: Vec<(&String, &(u32, u32))> = c.texture_sizes.iter().collect();
    sized.sort();
    w.u32(sized.len() as u32);
    for (stem, (width, height)) in sized {
        w.str(stem);
        w.u32(*width);
        w.u32(*height);
    }
    let mut flips: Vec<(&String, &TileRef)> = c.flipbooks.iter().collect();
    flips.sort();
    w.u32(flips.len() as u32);
    for (stem, t) in flips {
        w.str(stem);
        w.tile(t);
    }
    let mut tiles: Vec<(&String, &Vec<TileRef>)> = c.terrain_tiles.iter().collect();
    tiles.sort();
    w.u32(tiles.len() as u32);
    for (stem, ts) in tiles {
        w.str(stem);
        w.u32(ts.len() as u32);
        for t in ts {
            w.tile(t);
        }
    }
    let mut sounds: Vec<(&String, &String)> = c.sound_defs.iter().collect();
    sounds.sort();
    w.u32(sounds.len() as u32);
    for (event, def) in sounds {
        w.str(event);
        w.str(def);
    }
    let mut ids: Vec<&String> = c.render_defs.keys().collect();
    ids.sort();
    w.u32(ids.len() as u32);
    for id in ids {
        w.str(id);
        let defs = &c.render_defs[id];
        w.u32(defs.len() as u32);
        for d in defs {
            w.0.push(d.uv_anim as u8);
            w.u32(d.uses.len() as u32);
            for u in &d.uses {
                w.0.push(match u.slot {
                    Slot::Texture => 0,
                    Slot::Geometry => 1,
                    Slot::Material => 2,
                });
                w.str(&u.raw);
                w.0.push(u.per_bone as u8);
            }
            let mut names: Vec<&String> = d.arrays.keys().collect();
            names.sort();
            w.u32(names.len() as u32);
            for n in names {
                w.str(n);
                let elems = &d.arrays[n];
                w.u32(elems.len() as u32);
                for e in elems {
                    w.str(e);
                }
            }
        }
    }
    w.0
}

pub(super) fn decode(b: &[u8], expected_key: &str) -> Option<VanillaCatalog> {
    let mut r = Reader { b, i: 0 };
    if r.take(8)? != MAGIC || r.u32()? != FORMAT || r.str()? != expected_key {
        return None;
    }
    let mut c = VanillaCatalog {
        version: r.str()?,
        ..Default::default()
    };
    c.bundles_read = r.u32()? as usize;
    c.bundles_bad = r.u32()? as usize;
    c.textures = r.set()?;
    c.geometries = r.set()?;
    c.animations = r.set()?;
    c.animation_controllers = r.set()?;
    c.render_controllers = r.set()?;
    c.asset_stems = r.set()?;
    c.materials = r.set()?;
    c.item_atlas = r.set()?;
    let ns = r.count(12)?;
    let mut sizes = HashMap::with_capacity(ns);
    for _ in 0..ns {
        let stem = r.str()?;
        let size = (r.u32()?, r.u32()?);
        sizes.insert(stem, size);
    }
    c.texture_sizes = sizes;
    for _ in 0..r.count(13)? {
        let stem = r.str()?;
        c.flipbooks.insert(stem, r.tile()?);
    }
    for _ in 0..r.count(8)? {
        let stem = r.str()?;
        let nt = r.count(9)?;
        let mut ts = Vec::with_capacity(nt);
        for _ in 0..nt {
            ts.push(r.tile()?);
        }
        c.terrain_tiles.insert(stem, ts);
    }
    for _ in 0..r.count(8)? {
        let event = r.str()?;
        c.sound_defs.insert(event, r.str()?);
    }
    let n = r.count(4)?;
    let mut defs_by_id = HashMap::with_capacity(n);
    for _ in 0..n {
        let id = r.str()?;
        let nd = r.count(9)?;
        let mut defs = Vec::with_capacity(nd);
        for _ in 0..nd {
            let uv_anim = r.byte()? != 0;
            let nu = r.count(6)?;
            let mut uses = Vec::with_capacity(nu);
            for _ in 0..nu {
                let slot = match r.byte()? {
                    0 => Slot::Texture,
                    1 => Slot::Geometry,
                    2 => Slot::Material,
                    _ => return None,
                };
                let raw = r.str()?;
                let per_bone = r.byte()? != 0;
                uses.push(SlotUse {
                    slot,
                    raw,
                    per_bone,
                });
            }
            let na = r.count(8)?;
            let mut arrays = HashMap::with_capacity(na);
            for _ in 0..na {
                let name = r.str()?;
                let ne = r.count(4)?;
                let mut elems = Vec::with_capacity(ne);
                for _ in 0..ne {
                    elems.push(r.str()?);
                }
                arrays.insert(name, elems);
            }
            defs.push(RenderControllerDef {
                uses,
                arrays,
                uv_anim,
            });
        }
        defs_by_id.insert(id, defs);
    }
    c.render_defs = defs_by_id;
    // the whole file is the catalog: trailing bytes mean it is not ours
    (r.i == b.len()).then_some(c)
}

struct Writer(Vec<u8>);

impl Writer {
    fn u32(&mut self, v: u32) {
        self.0.extend(v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend(s.as_bytes());
    }
    fn tile(&mut self, t: &TileRef) {
        self.str(&t.tile);
        self.0.push(t.index.is_some() as u8);
        self.u32(t.index.unwrap_or(0));
    }
    /// Sorted, so the same catalog always gives the same bytes.
    fn set(&mut self, set: &HashSet<String>) {
        let mut v: Vec<&String> = set.iter().collect();
        v.sort();
        self.u32(v.len() as u32);
        for s in v {
            self.str(s);
        }
    }
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.i.checked_add(n)?;
        let s = self.b.get(self.i..end)?;
        self.i = end;
        Some(s)
    }
    fn byte(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    /// A count of items each taking at least `min_bytes`: refused if that many
    /// cannot fit in what is left, before anything is allocated.
    fn count(&mut self, min_bytes: usize) -> Option<usize> {
        let n = self.u32()? as usize;
        (n.checked_mul(min_bytes)? <= self.b.len() - self.i).then_some(n)
    }
    fn str(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
    fn tile(&mut self) -> Option<TileRef> {
        let tile = self.str()?;
        let has = match self.byte()? {
            0 => false,
            1 => true,
            _ => return None,
        };
        let index = self.u32()?;
        Some(TileRef {
            tile,
            index: has.then_some(index),
        })
    }
    fn set(&mut self) -> Option<HashSet<String>> {
        let n = self.count(4)?;
        let mut s = HashSet::with_capacity(n);
        for _ in 0..n {
            s.insert(self.str()?);
        }
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> VanillaCatalog {
        let mut c = VanillaCatalog {
            version: "1.26.52".into(),
            bundles_read: 3,
            ..Default::default()
        };
        c.textures.insert("textures/entity/cow/cow.png".into());
        c.geometries.insert("geometry.cow".into());
        c.materials.insert("entity_alphatest".into());
        c.asset_stems.insert("sounds/mob/cow/say1".into());
        c.texture_sizes
            .insert("textures/entity/cow/cow".into(), (64, 32));
        c.flipbooks.insert(
            "textures/blocks/lava_still".into(),
            TileRef {
                tile: "still_lava".into(),
                index: None,
            },
        );
        c.terrain_tiles.insert(
            "textures/blocks/planks_oak".into(),
            vec![
                TileRef {
                    tile: "planks".into(),
                    index: Some(0),
                },
                TileRef {
                    tile: "oak_planks".into(),
                    index: None,
                },
            ],
        );
        c.item_atlas.insert("textures/items/apple".into());
        c.sound_defs.insert(
            "mob.cow.say".into(),
            r#"{"category":"neutral","sounds":["sounds/mob/cow/say1"]}"#.into(),
        );
        let mut arrays = HashMap::new();
        arrays.insert(
            "array.skins".into(),
            vec!["Texture.default".into(), "Texture.ñandú".into()],
        );
        c.render_defs.insert(
            "controller.render.cow".into(),
            vec![RenderControllerDef {
                uses: vec![SlotUse {
                    slot: Slot::Material,
                    raw: "Material.default".into(),
                    per_bone: true,
                }],
                arrays,
                uv_anim: true,
            }],
        );
        c
    }

    #[test]
    fn round_trip_is_exact() {
        let c = sample();
        let bytes = encode(&c, "k");
        assert_eq!(decode(&bytes, "k"), Some(c));
        assert_eq!(encode(&sample(), "k"), bytes, "same catalog, same bytes");
    }

    /// The installed game's whole catalog survives the cache exactly. Skipped
    /// where the game is not installed.
    #[test]
    fn the_real_catalog_round_trips() {
        let Some(install) = crate::vanilla::install_dir() else {
            return;
        };
        let c = VanillaCatalog::from_install(&install);
        assert!(c.texture_count() > 1000);
        let back = decode(&encode(&c, "k"), "k").expect("decodes");
        assert!(back == c, "the cached catalog differs from the built one");
    }

    #[test]
    fn another_key_or_damage_means_rebuild() {
        let bytes = encode(&sample(), "k");
        assert_eq!(decode(&bytes, "other key"), None, "game updated: not used");
        for cut in 0..bytes.len() {
            assert_eq!(decode(&bytes[..cut], "k"), None, "truncated at {cut}");
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert_eq!(decode(&extra, "k"), None, "trailing bytes");
        // a count claiming billions of items
        let mut huge = MAGIC.to_vec();
        huge.extend(FORMAT.to_le_bytes());
        huge.extend(1u32.to_le_bytes());
        huge.extend(b"k");
        huge.extend(0u32.to_le_bytes()); // version ""
        huge.extend([0u8; 8]); // bundle counts
        huge.extend(u32::MAX.to_le_bytes()); // textures: 4 billion
        assert_eq!(decode(&huge, "k"), None);
    }
}
