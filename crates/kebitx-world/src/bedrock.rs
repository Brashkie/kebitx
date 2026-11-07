//! What a Bedrock world has saved: chunks, blocks, entities and block entities.
//!
//! A key says what its value is. Named ones (`~local_player`, `actorprefix...`) are
//! looked at first; the rest of the keys are chunk coordinates: x and z, the dimension
//! unless it is the overworld, a tag, and for a part of a chunk its height.
//!
//! What is not understood is counted as such: a format it does not know is never
//! taken for an empty chunk.

use crate::leveldb::{Damage, Db};
use kebitx_core::filesystem::read_limited;
use kebitx_core::nbt::{parse_level_dat, read_compound, Nbt};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

const SUBCHUNK_BLOCKS: usize = 4096;
const TAG_BLOCK_ENTITIES: u8 = 49;
const TAG_ENTITIES: u8 = 50;
const TAG_SUBCHUNK: u8 = 47;
/// What a chunk's tag can be: anything else with the same shape is not a chunk.
const CHUNK_TAGS: std::ops::RangeInclusive<u8> = 43..=65;
const OLD_VERSION_TAG: u8 = 118;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub min_x: i32,
    pub max_x: i32,
    pub min_z: i32,
    pub max_z: i32,
}

#[derive(Debug, Default)]
pub struct Dimension {
    pub chunks: u64,
    pub subchunks: u64,
    pub bounds: Bounds,
}

/// Bytes each kind of thing takes, uncompressed.
#[derive(Debug, Default)]
pub struct Sizes {
    pub terrain: u64,
    pub entities: u64,
    pub block_entities: u64,
    pub players: u64,
    pub other: u64,
}

#[derive(Debug, Default)]
pub struct World {
    pub name: String,
    pub version: Option<String>,
    pub seed: Option<i64>,
    /// By dimension number: 0 overworld, 1 nether, 2 the end.
    pub dimensions: BTreeMap<i32, Dimension>,
    /// Block id -> how many, from each chunk part's first layer.
    pub blocks: BTreeMap<String, u64>,
    pub entities: BTreeMap<String, u64>,
    pub block_entities: BTreeMap<String, u64>,
    pub players: u64,
    pub sizes: Sizes,
    pub disk_bytes: u64,
    /// Chunk parts in an old format, not counted in `blocks`.
    pub old_subchunks: u64,
    /// Values that could not be understood.
    pub unreadable: u64,
    pub damage: Damage,
}

impl World {
    pub fn total_blocks(&self) -> u64 {
        self.blocks.values().sum()
    }
}

/// What the name of a key says it holds.
#[derive(Debug, PartialEq, Eq)]
enum Key {
    Actor,
    Player,
    /// Lists of which actors are in a chunk: no data of their own.
    ActorIndex,
    Named,
    Chunk {
        x: i32,
        z: i32,
        dim: i32,
        tag: u8,
    },
}

fn i32_at(b: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn classify(key: &[u8]) -> Key {
    if key.starts_with(b"actorprefix") {
        return Key::Actor;
    }
    if key.starts_with(b"digp") {
        return Key::ActorIndex;
    }
    if key.starts_with(b"player") || key == b"~local_player" {
        return Key::Player;
    }
    if key.starts_with(b"map_") || key.starts_with(b"VILLAGE_") || key.starts_with(b"structure") {
        return Key::Named;
    }
    let shape = match key.len() {
        9 | 10 => Some((0, 8)),
        13 | 14 => i32_at(key, 8).map(|dim| (dim, 12)),
        _ => None,
    };
    if let Some((dim, tag_at)) = shape {
        let tag = key[tag_at];
        if CHUNK_TAGS.contains(&tag) || tag == OLD_VERSION_TAG {
            if let (Some(x), Some(z)) = (i32_at(key, 0), i32_at(key, 4)) {
                return Key::Chunk { x, z, dim, tag };
            }
        }
    }
    Key::Named
}

/// Each compound in a row, as the game saves block entities.
fn compounds(mut value: &[u8], mut each: impl FnMut(&Nbt)) -> bool {
    while !value.is_empty() {
        let Ok((nbt, used)) = read_compound(value) else {
            return false;
        };
        each(&nbt);
        value = &value[used..];
    }
    true
}

fn text_of<'a>(nbt: &'a Nbt, key: &str) -> Option<&'a str> {
    match nbt.get(key)? {
        Nbt::String(s) => Some(s),
        _ => None,
    }
}

/// Counts the blocks of a chunk part. `Err` is damage; `Ok(false)`, a format not
/// known here.
fn count_subchunk(value: &[u8], blocks: &mut BTreeMap<String, u64>) -> Result<bool, ()> {
    let (&version, _) = value.split_first().ok_or(())?;
    let mut at = match version {
        1 => 1,
        8 => 2,
        9 => 3,
        _ => return Ok(false),
    };
    if version != 1 && *value.get(1).ok_or(())? == 0 {
        return Ok(true); // no layers: nothing but air, and nothing saved
    }
    // the first layer holds the blocks; a second one, what is inside them (water)
    let header = *value.get(at).ok_or(())?;
    at += 1;
    if header & 1 != 0 {
        return Err(()); // runtime ids mean nothing outside a running game
    }
    let bits = usize::from(header >> 1);
    if !matches!(bits, 0 | 1 | 2 | 3 | 4 | 5 | 6 | 8 | 16) {
        return Err(());
    }
    let mut counts: Vec<u64> = Vec::new();
    let mut words_at = 0;
    let mut word_count = 0;
    // no bits at all: every block is the palette's only entry, and no words are saved
    let per_word = 32usize.checked_div(bits);
    if let Some(per_word) = per_word {
        word_count = SUBCHUNK_BLOCKS.div_ceil(per_word);
        words_at = at;
        at = at.checked_add(word_count * 4).ok_or(())?;
        value.get(words_at..at).ok_or(())?;
    }
    // a uniform part may or may not say its palette has one entry
    let palette_size = if bits == 0 && value.get(at) == Some(&10) {
        1
    } else {
        let n = i32_at(value, at).ok_or(())?;
        at += 4;
        usize::try_from(n).map_err(|_| ())?
    };
    if palette_size == 0 || palette_size > SUBCHUNK_BLOCKS {
        return Err(());
    }
    let mut names: Vec<String> = Vec::with_capacity(palette_size);
    for _ in 0..palette_size {
        let (nbt, used) = read_compound(value.get(at..).ok_or(())?).map_err(|_| ())?;
        names.push(text_of(&nbt, "name").ok_or(())?.to_string());
        at += used;
    }
    counts.resize(palette_size, 0);
    if let Some(per_word) = per_word {
        let mask = (1u32 << bits) - 1;
        let mut left = SUBCHUNK_BLOCKS;
        let (words, _) = value[words_at..words_at + word_count * 4].as_chunks::<4>();
        for word in words {
            let mut w = u32::from_le_bytes(*word);
            for _ in 0..per_word.min(left) {
                *counts.get_mut((w & mask) as usize).ok_or(())? += 1;
                w >>= bits;
            }
            left = left.saturating_sub(per_word);
        }
    } else {
        counts[0] = SUBCHUNK_BLOCKS as u64;
    }
    for (name, count) in names.into_iter().zip(counts) {
        if count > 0 {
            *blocks.entry(name).or_default() += count;
        }
    }
    Ok(true)
}

fn read_level_dat(world_dir: &Path, world: &mut World) {
    world.name = world_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let Ok(bytes) = read_limited(&world_dir.join("level.dat"), 16 << 20) else {
        return;
    };
    let Ok(root) = parse_level_dat(&bytes) else {
        return;
    };
    if let Some(name) = text_of(&root, "LevelName").filter(|n| !n.trim().is_empty()) {
        world.name = name.to_string();
    }
    world.seed = root.get("RandomSeed").and_then(Nbt::as_i64);
    if let Some(Nbt::List(parts)) = root.get("lastOpenedWithVersion") {
        let numbers: Vec<String> = parts
            .iter()
            .filter_map(Nbt::as_i64)
            .map(|n| n.to_string())
            .collect();
        // the game pads it with zeros
        let wanted = numbers.len().min(3);
        if wanted > 0 {
            world.version = Some(numbers[..wanted].join("."));
        }
    }
}

/// Reads a whole world folder. `step` is told how far along it is, and false stops
/// it: then `Ok(None)`. Reads, and never writes.
pub fn inspect(
    world_dir: &Path,
    step: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Option<World>, String> {
    let mut db = Db::open(&world_dir.join("db"))?;
    let mut world = World {
        disk_bytes: db.disk_bytes,
        ..Default::default()
    };
    read_level_dat(world_dir, &mut world);
    let mut chunks: HashSet<(i32, i32, i32)> = HashSet::new();
    let finished = db.for_each(
        &mut |key, value| {
            let size = value.len() as u64;
            match classify(key) {
                Key::Actor => {
                    world.sizes.entities += size;
                    match read_compound(value) {
                        Ok((nbt, _)) => {
                            let id = text_of(&nbt, "identifier").unwrap_or("?");
                            *world.entities.entry(id.to_string()).or_default() += 1;
                        }
                        Err(_) => world.unreadable += 1,
                    }
                }
                Key::ActorIndex => world.sizes.entities += size,
                Key::Player => {
                    world.sizes.players += size;
                    world.players += 1;
                }
                Key::Named => world.sizes.other += size,
                Key::Chunk { x, z, dim, tag } => {
                    let dimension = world.dimensions.entry(dim).or_default();
                    if chunks.insert((dim, x, z)) {
                        let b = &mut dimension.bounds;
                        if dimension.chunks == 0 {
                            *b = Bounds {
                                min_x: x,
                                max_x: x,
                                min_z: z,
                                max_z: z,
                            };
                        } else {
                            b.min_x = b.min_x.min(x);
                            b.max_x = b.max_x.max(x);
                            b.min_z = b.min_z.min(z);
                            b.max_z = b.max_z.max(z);
                        }
                        dimension.chunks += 1;
                    }
                    match tag {
                        TAG_SUBCHUNK => {
                            world.sizes.terrain += size;
                            dimension.subchunks += 1;
                            match count_subchunk(value, &mut world.blocks) {
                                Ok(true) => {}
                                Ok(false) => world.old_subchunks += 1,
                                Err(()) => world.unreadable += 1,
                            }
                        }
                        TAG_BLOCK_ENTITIES => {
                            world.sizes.block_entities += size;
                            let map = &mut world.block_entities;
                            let whole = compounds(value, |nbt| {
                                let id = text_of(nbt, "id").unwrap_or("?");
                                *map.entry(id.to_string()).or_default() += 1;
                            });
                            if !whole {
                                world.unreadable += 1;
                            }
                        }
                        TAG_ENTITIES => {
                            world.sizes.entities += size;
                            let map = &mut world.entities;
                            let whole = compounds(value, |nbt| {
                                let id = text_of(nbt, "identifier").unwrap_or("?");
                                *map.entry(id.to_string()).or_default() += 1;
                            });
                            if !whole {
                                world.unreadable += 1;
                            }
                        }
                        _ => world.sizes.terrain += size,
                    }
                }
            }
        },
        step,
    );
    if !finished {
        return Ok(None);
    }
    world.damage = db.damage.clone();
    Ok(Some(world))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::leveldb::tests::{batch, db_folder, log_file, table_file};

    /// A compound as the game saves it: strings and an int are all that is needed.
    fn nbt(fields: &[(&str, &str)]) -> Vec<u8> {
        let mut out = vec![10, 0, 0];
        for (key, value) in fields {
            out.push(8);
            out.extend_from_slice(&(key.len() as u16).to_le_bytes());
            out.extend_from_slice(key.as_bytes());
            out.extend_from_slice(&(value.len() as u16).to_le_bytes());
            out.extend_from_slice(value.as_bytes());
        }
        out.push(0);
        out
    }

    fn chunk_key(x: i32, z: i32, dim: i32, tag: u8, y: Option<i8>) -> Vec<u8> {
        let mut key = Vec::new();
        key.extend_from_slice(&x.to_le_bytes());
        key.extend_from_slice(&z.to_le_bytes());
        if dim != 0 {
            key.extend_from_slice(&dim.to_le_bytes());
        }
        key.push(tag);
        if let Some(y) = y {
            key.push(y as u8);
        }
        key
    }

    /// A chunk part with `palette`, whose first `stone` blocks are palette 1.
    fn subchunk(palette: &[&str], stone: usize, version: u8) -> Vec<u8> {
        let mut out = vec![version, 1];
        if version == 9 {
            out.push(4);
        }
        out.push(1 << 1); // one bit per block, saved with names
        for word in 0..128 {
            let mut w = 0u32;
            for bit in 0..32 {
                if word * 32 + bit < stone {
                    w |= 1 << bit;
                }
            }
            out.extend_from_slice(&w.to_le_bytes());
        }
        out.extend_from_slice(&(palette.len() as i32).to_le_bytes());
        for name in palette {
            out.extend(nbt(&[("name", name)]));
        }
        out
    }

    #[test]
    fn counts_what_a_world_has() {
        let part = subchunk(&["minecraft:air", "minecraft:stone"], 100, 9);
        let uniform = {
            let mut out = vec![9, 1, 0, 0];
            out.extend_from_slice(&1i32.to_le_bytes());
            out.extend(nbt(&[("name", "minecraft:dirt")]));
            out
        };
        let uniform_no_count = {
            let mut out = vec![8, 1, 0];
            out.extend(nbt(&[("name", "minecraft:sand")]));
            out
        };
        let mut chests = nbt(&[("id", "Chest")]);
        chests.extend(nbt(&[("id", "Chest")]));
        chests.extend(nbt(&[("id", "Sign")]));
        let actor = nbt(&[("identifier", "minecraft:zombie")]);
        let keys = [
            (chunk_key(1, 2, 0, 47, Some(4)), part.clone()),
            (chunk_key(1, 2, 0, 47, Some(5)), uniform),
            (chunk_key(1, 2, 0, 47, Some(6)), uniform_no_count),
            (chunk_key(1, 2, 0, 44, None), vec![40]),
            (chunk_key(-3, 7, 0, 44, None), vec![40]),
            (chunk_key(9, 9, 1, 47, Some(0)), part),
            (chunk_key(1, 2, 0, 49, None), chests),
            (b"actorprefix\0\0\0\0\0\0\0\x01".to_vec(), actor.clone()),
            (b"actorprefix\0\0\0\0\0\0\0\x02".to_vec(), actor),
            (b"digp\x01\0\0\0\x02\0\0\0".to_vec(), vec![0; 8]),
            (b"~local_player".to_vec(), vec![1, 2, 3]),
            (b"player_server_abc".to_vec(), vec![4]),
            // the shape of a chunk key, and text
            (b"Overworld".to_vec(), vec![5]),
            (b"map_123456".to_vec(), vec![6]),
            // a format not known here is said, not taken for an empty part
            (chunk_key(1, 2, 0, 47, Some(7)), vec![2, 0, 0]),
            // and damage is said too
            (chunk_key(1, 2, 0, 47, Some(8)), vec![9, 1, 4, 3]),
        ];
        let entries: Vec<(&[u8], u64, bool, Vec<u8>)> = keys
            .iter()
            .enumerate()
            .map(|(i, (k, v))| (k.as_slice(), i as u64 + 1, true, v.clone()))
            .collect();
        let dir = db_folder("world", &[(1, 2, table_file(&entries, 4))], None);
        let world_dir = dir.with_extension("world");
        let _ = std::fs::remove_dir_all(&world_dir);
        std::fs::create_dir_all(&world_dir).unwrap();
        std::fs::rename(&dir, world_dir.join("db")).unwrap();

        let w = inspect(&world_dir, &mut |_, _| true).unwrap().unwrap();
        assert_eq!(w.blocks["minecraft:stone"], 200);
        assert_eq!(w.blocks["minecraft:air"], 2 * (4096 - 100));
        assert_eq!(w.blocks["minecraft:dirt"], 4096);
        assert_eq!(w.blocks["minecraft:sand"], 4096);
        // every block of every part understood is counted
        assert_eq!(w.total_blocks(), 4 * 4096);
        assert_eq!(w.old_subchunks, 1);
        assert_eq!(w.unreadable, 1);
        assert_eq!(w.entities["minecraft:zombie"], 2);
        assert_eq!(w.block_entities["Chest"], 2);
        assert_eq!(w.block_entities["Sign"], 1);
        assert_eq!(w.players, 2);
        let over = &w.dimensions[&0];
        assert_eq!(over.chunks, 2);
        assert_eq!(over.subchunks, 5);
        assert_eq!(
            over.bounds,
            Bounds {
                min_x: -3,
                max_x: 1,
                min_z: 2,
                max_z: 7
            }
        );
        assert_eq!(w.dimensions[&1].chunks, 1);
        assert!(w.sizes.other >= 2);
        std::fs::remove_dir_all(&world_dir).unwrap();
    }

    #[test]
    fn what_a_log_rewrote_is_counted_once() {
        let old = subchunk(&["minecraft:air", "minecraft:stone"], 4096, 8);
        let new = subchunk(&["minecraft:air", "minecraft:dirt"], 4096, 8);
        let key = chunk_key(0, 0, 0, 47, Some(0));
        let table = table_file(&[(key.as_slice(), 1, true, old)], 0);
        let log = log_file(&[batch(&[(key.as_slice(), Some(new.as_slice()))])]);
        let dir = db_folder("rewrite", &[(1, 2, table)], Some(log));
        let world_dir = dir.with_extension("world");
        let _ = std::fs::remove_dir_all(&world_dir);
        std::fs::create_dir_all(&world_dir).unwrap();
        std::fs::rename(&dir, world_dir.join("db")).unwrap();
        let w = inspect(&world_dir, &mut |_, _| true).unwrap().unwrap();
        assert_eq!(w.blocks.get("minecraft:stone"), None);
        assert_eq!(w.blocks["minecraft:dirt"], 4096);
        std::fs::remove_dir_all(&world_dir).unwrap();
    }

    #[test]
    fn a_folder_with_no_database_is_an_error_not_an_empty_world() {
        assert!(inspect(Path::new("no/such/world"), &mut |_, _| true).is_err());
    }

    #[test]
    fn any_bytes_in_a_chunk_part_are_only_an_error() {
        let good = subchunk(&["minecraft:air", "minecraft:stone"], 7, 9);
        let mut seed = 0x9E37_79B9u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for _ in 0..4000 {
            let mut changed = good.clone();
            for _ in 0..1 + next() % 6 {
                let at = next() as usize % changed.len();
                changed[at] = next() as u8;
            }
            let cut = next() as usize % (changed.len() + 1);
            let mut blocks = BTreeMap::new();
            let _ = count_subchunk(&changed[..cut], &mut blocks);
            let _ = count_subchunk(&changed, &mut blocks);
            let _ = classify(&changed[..cut.min(20)]);
        }
    }

    /// A real world, when one is named. It never changes what it reads.
    #[test]
    fn a_real_world_when_there_is_one() {
        let Some(path) = std::env::var_os("KEBITX_TEST_WORLD") else {
            return;
        };
        let w = inspect(Path::new(&path), &mut |_, _| true)
            .unwrap()
            .unwrap();
        assert_eq!(w.damage, Damage::default());
        assert_eq!(w.unreadable, 0);
        let parts: u64 = w.dimensions.values().map(|d| d.subchunks).sum();
        // every part read has exactly its 4096 blocks
        assert_eq!(w.total_blocks(), (parts - w.old_subchunks) * 4096);
    }
}
