//! The database Bedrock keeps a world in: LevelDB, as Mojang changed it. Read only.
//!
//! `CURRENT` names a `MANIFEST`, whose edits say which `.ldb` tables are alive. The
//! newest writes are still in `.log` files. A key may be in many places: its newest
//! version is the one that counts, and a deletion hides all those before it. So the
//! sources are read newest first (logs, level 0 by number, then the deeper levels),
//! and only the first time a key is seen matters.
//!
//! Mojang compresses blocks with zlib (2) or raw DEFLATE (4). Snappy (1), which it
//! does not use, is counted as unreadable. Each block's checksum is verified.

use kebitx_core::codec::inflate::{inflate, zlib_decompress};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

/// More than any block or file of a real world: past this it is damage or an attack.
const MAX_BLOCK: usize = 64 << 20;
const MAX_FILE: u64 = 1 << 30;

const TABLE_MAGIC: u64 = 0xdb47_7524_8b80_fb57;
const FOOTER: usize = 48;
const BLOCK_TRAILER: usize = 5;
const LOG_BLOCK: usize = 32768;
const LOG_HEADER: usize = 7;

/// What could not be read: each one hides what was in it, so each one is said.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Damage {
    pub tables: usize,
    pub blocks: usize,
    pub missing_tables: usize,
}

pub struct Db {
    /// The live tables, newest first.
    tables: Vec<PathBuf>,
    /// The logs still in use, oldest first: each one rewrites the one before.
    logs: Vec<PathBuf>,
    pub damage: Damage,
    pub disk_bytes: u64,
}

// ------------------------------------------------------------------ pieces

const fn crc_table() -> [u32; 256] {
    // CRC-32C, the Castagnoli polynomial, as LevelDB uses
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut bit = 0;
        while bit < 8 {
            c = if c & 1 != 0 {
                0x82f6_3b78 ^ (c >> 1)
            } else {
                c >> 1
            };
            bit += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}
static CRC: [u32; 256] = crc_table();

fn crc32c(parts: &[&[u8]]) -> u32 {
    let mut c = !0u32;
    for part in parts {
        for &b in *part {
            c = CRC[((c ^ u32::from(b)) & 255) as usize] ^ (c >> 8);
        }
    }
    !c
}

/// As LevelDB stores a checksum, so that a checksum of checksums means nothing.
pub fn masked_crc(parts: &[&[u8]]) -> u32 {
    crc32c(parts).rotate_right(15).wrapping_add(0xa282_ead8)
}

/// A varint at `*at`, moved past it. `None` when it is cut short or too long.
fn varint(b: &[u8], at: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *b.get(*at)?;
        *at += 1;
        v |= u64::from(byte & 127) << shift;
        if byte < 128 {
            return Some(v);
        }
    }
    None
}

fn read_file(path: &Path) -> Option<Vec<u8>> {
    let size = std::fs::metadata(path).ok()?.len();
    if size > MAX_FILE {
        return None;
    }
    std::fs::read(path).ok()
}

/// The whole records of a log, or of a manifest, which is written the same way. A
/// tail cut short or damaged ends it: the game writes it last and may have stopped.
pub fn log_records(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut partial: Vec<u8> = Vec::new();
    let mut at = 0;
    while at + LOG_HEADER <= bytes.len() {
        let room = LOG_BLOCK - at % LOG_BLOCK;
        if room < LOG_HEADER {
            at += room; // the end of a block too small for a header
            continue;
        }
        let stored = u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
        let len = usize::from(u16::from_le_bytes([bytes[at + 4], bytes[at + 5]]));
        let kind = bytes[at + 6];
        if kind == 0 && len == 0 {
            at += room; // padding
            continue;
        }
        let Some(data) = bytes.get(at + LOG_HEADER..at + LOG_HEADER + len) else {
            break;
        };
        if len > room - LOG_HEADER || masked_crc(&[&[kind], data]) != stored {
            break;
        }
        match kind {
            1 => out.push(data.to_vec()),
            2 => partial = data.to_vec(),
            3 => partial.extend_from_slice(data),
            4 => {
                partial.extend_from_slice(data);
                out.push(std::mem::take(&mut partial));
            }
            _ => break,
        }
        at += LOG_HEADER + len;
    }
    out
}

// ------------------------------------------------------------------ opening

/// The tables alive and the first log in use, from a manifest's edits.
struct Version {
    files: BTreeMap<u32, BTreeSet<u64>>,
    log_number: u64,
}

fn apply_edit(edit: &[u8], v: &mut Version) -> Option<()> {
    let mut at = 0;
    let bytes = |at: &mut usize| -> Option<()> {
        let n = varint(edit, at)? as usize;
        edit.get(*at..at.checked_add(n)?)?;
        *at += n;
        Some(())
    };
    while at < edit.len() {
        match varint(edit, &mut at)? {
            1 => bytes(&mut at)?, // comparator
            2 => v.log_number = varint(edit, &mut at)?,
            3 | 4 | 9 => {
                varint(edit, &mut at)?; // next file, last sequence, previous log
            }
            5 => {
                varint(edit, &mut at)?; // compaction pointer: level, key
                bytes(&mut at)?;
            }
            6 => {
                let level = varint(edit, &mut at)? as u32;
                let file = varint(edit, &mut at)?;
                if let Some(files) = v.files.get_mut(&level) {
                    files.remove(&file);
                }
            }
            7 => {
                let level = varint(edit, &mut at)? as u32;
                let file = varint(edit, &mut at)?;
                varint(edit, &mut at)?; // size
                bytes(&mut at)?; // smallest key
                bytes(&mut at)?; // largest key
                v.files.entry(level).or_default().insert(file);
            }
            _ => return None,
        }
    }
    Some(())
}

impl Db {
    /// Opens the `db` folder of a world. Reads, and creates nothing.
    pub fn open(dir: &Path) -> Result<Db, String> {
        let current =
            std::fs::read_to_string(dir.join("CURRENT")).map_err(|e| format!("CURRENT: {e}"))?;
        let name = current.trim();
        if name.is_empty() || name.contains(['/', '\\']) || name.contains("..") {
            return Err("CURRENT does not name a manifest".to_string());
        }
        let manifest =
            read_file(&dir.join(name)).ok_or_else(|| format!("{name}: cannot be read"))?;
        let mut version = Version {
            files: BTreeMap::new(),
            log_number: 0,
        };
        let records = log_records(&manifest);
        if records.is_empty() {
            return Err(format!("{name}: no edits"));
        }
        for edit in records {
            apply_edit(&edit, &mut version).ok_or_else(|| format!("{name}: a damaged edit"))?;
        }

        let mut damage = Damage::default();
        let path_of = |number: u64| {
            let new = dir.join(format!("{number:06}.ldb"));
            // the original LevelDB's name for a table
            if new.is_file() {
                new
            } else {
                dir.join(format!("{number:06}.sst"))
            }
        };
        let mut tables = Vec::new();
        for (level, files) in &version.files {
            let in_order: Vec<u64> = if *level == 0 {
                files.iter().rev().copied().collect()
            } else {
                files.iter().copied().collect()
            };
            for number in in_order {
                let path = path_of(number);
                if path.is_file() {
                    tables.push(path);
                } else {
                    damage.missing_tables += 1;
                }
            }
        }
        let mut logs: Vec<(u64, PathBuf)> = Vec::new();
        let mut disk_bytes = 0;
        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
            let path = entry.path();
            disk_bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
            if path.extension().is_some_and(|e| e == "log") {
                let number = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse().ok());
                if let Some(number) = number.filter(|n| *n >= version.log_number) {
                    logs.push((number, path));
                }
            }
        }
        logs.sort();
        Ok(Db {
            tables,
            logs: logs.into_iter().map(|(_, p)| p).collect(),
            damage,
            disk_bytes,
        })
    }

    /// How many steps `for_each` tells: the logs, and each table.
    pub fn steps(&self) -> usize {
        self.tables.len() + 1
    }

    /// Every key alive and its value, in no useful order. `step` is told each table
    /// about to be read, and false stops it there: false is returned then.
    pub fn for_each(
        &mut self,
        visit: &mut dyn FnMut(&[u8], &[u8]),
        step: &mut dyn FnMut(usize, usize) -> bool,
    ) -> bool {
        let total = self.steps();
        if !step(0, total) {
            return false;
        }
        let mut seen: HashSet<Vec<u8>> = HashSet::new();
        // the logs: the last write of each key, deletions included
        let mut written: BTreeMap<Vec<u8>, Option<Vec<u8>>> = BTreeMap::new();
        for log in &self.logs {
            let Some(bytes) = read_file(log) else {
                continue;
            };
            for batch in log_records(&bytes) {
                apply_batch(&batch, &mut written);
            }
        }
        for (key, value) in written {
            if let Some(value) = &value {
                visit(&key, value);
            }
            seen.insert(key);
        }
        let tables = std::mem::take(&mut self.tables);
        for (i, path) in tables.iter().enumerate() {
            if !step(i + 1, total) {
                self.tables = tables;
                return false;
            }
            let Some(bytes) = read_file(path) else {
                self.damage.tables += 1;
                continue;
            };
            read_table(&bytes, &mut seen, visit, &mut self.damage);
        }
        self.tables = tables;
        true
    }
}

/// A write batch: its sequence and count, then puts and deletes. Past damage,
/// nothing more of it can be trusted.
fn apply_batch(batch: &[u8], written: &mut BTreeMap<Vec<u8>, Option<Vec<u8>>>) -> Option<()> {
    let mut at = 12usize;
    let text = |at: &mut usize| -> Option<Vec<u8>> {
        let n = varint(batch, at)? as usize;
        let out = batch.get(*at..at.checked_add(n)?)?.to_vec();
        *at += n;
        Some(out)
    };
    while at < batch.len() {
        let kind = *batch.get(at)?;
        at += 1;
        match kind {
            1 => {
                let key = text(&mut at)?;
                let value = text(&mut at)?;
                written.insert(key, Some(value));
            }
            0 => {
                let key = text(&mut at)?;
                written.insert(key, None);
            }
            _ => return None,
        }
    }
    Some(())
}

// ------------------------------------------------------------------ tables

/// A block's bytes, once its checksum holds and it is uncompressed.
fn read_block(file: &[u8], offset: u64, size: u64) -> Option<Vec<u8>> {
    let start = usize::try_from(offset).ok()?;
    let size = usize::try_from(size).ok()?;
    let end = start.checked_add(size)?;
    let body = file.get(start..end)?;
    let trailer = file.get(end..end.checked_add(BLOCK_TRAILER)?)?;
    let stored = u32::from_le_bytes([trailer[1], trailer[2], trailer[3], trailer[4]]);
    if masked_crc(&[body, &trailer[..1]]) != stored {
        return None;
    }
    match trailer[0] {
        0 => Some(body.to_vec()),
        2 => zlib_decompress(body, MAX_BLOCK).ok(),
        4 => inflate(body, MAX_BLOCK).ok().map(|(bytes, _)| bytes),
        _ => None,
    }
}

/// Each entry of a block: `visit(key, value)`. `None` when it is damaged.
fn block_entries(block: &[u8], visit: &mut dyn FnMut(&[u8], &[u8])) -> Option<()> {
    let count_at = block.len().checked_sub(4)?;
    let restarts = u32::from_le_bytes(block[count_at..].try_into().ok()?) as usize;
    let end = count_at.checked_sub(restarts.checked_mul(4)?)?;
    let mut key: Vec<u8> = Vec::new();
    let mut at = 0;
    while at < end {
        let shared = varint(block, &mut at)? as usize;
        let own = varint(block, &mut at)? as usize;
        let value_len = varint(block, &mut at)? as usize;
        if shared > key.len() {
            return None;
        }
        let own_end = at.checked_add(own)?;
        let value_end = own_end.checked_add(value_len)?;
        if value_end > end {
            return None;
        }
        key.truncate(shared);
        key.extend_from_slice(&block[at..own_end]);
        visit(&key, &block[own_end..value_end]);
        at = value_end;
    }
    Some(())
}

fn read_table(
    file: &[u8],
    seen: &mut HashSet<Vec<u8>>,
    visit: &mut dyn FnMut(&[u8], &[u8]),
    damage: &mut Damage,
) {
    let Some(index) = table_index(file) else {
        damage.tables += 1;
        return;
    };
    let mut handles: Vec<(u64, u64)> = Vec::new();
    let read = block_entries(&index, &mut |_, value| {
        let mut at = 0;
        if let (Some(offset), Some(size)) = (varint(value, &mut at), varint(value, &mut at)) {
            handles.push((offset, size));
        }
    });
    if read.is_none() {
        damage.tables += 1;
    }
    for (offset, size) in handles {
        let Some(block) = read_block(file, offset, size) else {
            damage.blocks += 1;
            continue;
        };
        let walked = block_entries(&block, &mut |internal, value| {
            // the key, then 8 bytes: the sequence, and what this entry is
            let Some(split) = internal.len().checked_sub(8) else {
                return;
            };
            let (key, tag) = internal.split_at(split);
            // in a table a key's newest version comes first, and newer tables came
            // before this one: only the first time counts
            if !seen.contains(key) {
                seen.insert(key.to_vec());
                if tag[0] == 1 {
                    visit(key, value);
                }
            }
        });
        if walked.is_none() {
            damage.blocks += 1;
        }
    }
}

fn table_index(file: &[u8]) -> Option<Vec<u8>> {
    let footer = file.get(file.len().checked_sub(FOOTER)?..)?;
    let magic = u64::from_le_bytes(footer[FOOTER - 8..].try_into().ok()?);
    if magic != TABLE_MAGIC {
        return None;
    }
    let mut at = 0;
    varint(footer, &mut at)?; // the metaindex: filters, which nothing here needs
    varint(footer, &mut at)?;
    let offset = varint(footer, &mut at)?;
    let size = varint(footer, &mut at)?;
    read_block(file, offset, size)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn put_varint(out: &mut Vec<u8>, mut v: u64) {
        while v >= 128 {
            out.push((v & 127) as u8 | 128);
            v >>= 7;
        }
        out.push(v as u8);
    }

    /// One log record of whichever kind: a full one, unless it does not fit.
    pub fn log_file(records: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        for record in records {
            let mut left: &[u8] = record;
            let mut first = true;
            loop {
                let room = LOG_BLOCK - out.len() % LOG_BLOCK;
                if room < LOG_HEADER {
                    out.resize(out.len() + room, 0);
                    continue;
                }
                let take = left.len().min(room - LOG_HEADER);
                let last = take == left.len();
                let kind: u8 = match (first, last) {
                    (true, true) => 1,
                    (true, false) => 2,
                    (false, false) => 3,
                    (false, true) => 4,
                };
                let (data, rest) = left.split_at(take);
                out.extend_from_slice(&masked_crc(&[&[kind], data]).to_le_bytes());
                out.extend_from_slice(&(take as u16).to_le_bytes());
                out.push(kind);
                out.extend_from_slice(data);
                left = rest;
                first = false;
                if last {
                    break;
                }
            }
        }
        out
    }

    /// `kind` 0 stores it as is, 4 as DEFLATE's stored blocks: both are real blocks.
    fn block_bytes(entries: &[(Vec<u8>, Vec<u8>)], kind: u8) -> Vec<u8> {
        let mut block = Vec::new();
        let mut last: Vec<u8> = Vec::new();
        for (key, value) in entries {
            let shared = key.iter().zip(&last).take_while(|(a, b)| a == b).count();
            put_varint(&mut block, shared as u64);
            put_varint(&mut block, (key.len() - shared) as u64);
            put_varint(&mut block, value.len() as u64);
            block.extend_from_slice(&key[shared..]);
            block.extend_from_slice(value);
            last = key.clone();
        }
        block.extend_from_slice(&0u32.to_le_bytes());
        block.extend_from_slice(&1u32.to_le_bytes());
        if kind == 4 {
            let mut out = Vec::new();
            for (i, chunk) in block.chunks(60000).enumerate() {
                let is_last = (i + 1) * 60000 >= block.len();
                out.push(u8::from(is_last));
                out.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
                out.extend_from_slice(&(!(chunk.len() as u16)).to_le_bytes());
                out.extend_from_slice(chunk);
            }
            return out;
        }
        block
    }

    fn add_block(file: &mut Vec<u8>, body: &[u8], kind: u8) -> (u64, u64) {
        let offset = file.len() as u64;
        file.extend_from_slice(body);
        file.push(kind);
        file.extend_from_slice(&masked_crc(&[body, &[kind]]).to_le_bytes());
        (offset, body.len() as u64)
    }

    /// A table of (key, sequence, is_put, value), in the order a table keeps them.
    pub fn table_file(entries: &[(&[u8], u64, bool, Vec<u8>)], kind: u8) -> Vec<u8> {
        let mut sorted: Vec<&(&[u8], u64, bool, Vec<u8>)> = entries.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(b.0).then(b.1.cmp(&a.1)));
        let internal: Vec<(Vec<u8>, Vec<u8>)> = sorted
            .iter()
            .map(|(key, seq, is_put, value)| {
                let mut k = key.to_vec();
                k.extend_from_slice(&((seq << 8) | u64::from(*is_put)).to_le_bytes());
                (k, value.clone())
            })
            .collect();
        let mut file = Vec::new();
        let (offset, size) = add_block(&mut file, &block_bytes(&internal, kind), kind);
        let (meta_offset, meta_size) = add_block(&mut file, &block_bytes(&[], 0), 0);
        let mut handle = Vec::new();
        put_varint(&mut handle, offset);
        put_varint(&mut handle, size);
        let last_key = internal.last().map(|(k, _)| k.clone()).unwrap_or_default();
        let (index_offset, index_size) =
            add_block(&mut file, &block_bytes(&[(last_key, handle)], 0), 0);
        let mut footer = Vec::new();
        put_varint(&mut footer, meta_offset);
        put_varint(&mut footer, meta_size);
        put_varint(&mut footer, index_offset);
        put_varint(&mut footer, index_size);
        footer.resize(FOOTER - 8, 0);
        footer.extend_from_slice(&TABLE_MAGIC.to_le_bytes());
        file.extend_from_slice(&footer);
        file
    }

    pub fn batch(ops: &[(&[u8], Option<&[u8]>)]) -> Vec<u8> {
        let mut out = vec![0u8; 12];
        for (key, value) in ops {
            out.push(u8::from(value.is_some()));
            put_varint(&mut out, key.len() as u64);
            out.extend_from_slice(key);
            if let Some(value) = value {
                put_varint(&mut out, value.len() as u64);
                out.extend_from_slice(value);
            }
        }
        out
    }

    /// A database folder with these tables (level, number, bytes) and this log.
    pub fn db_folder(tag: &str, tables: &[(u64, u64, Vec<u8>)], log: Option<Vec<u8>>) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kebitx-ldb-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut edit = Vec::new();
        put_varint(&mut edit, 2);
        put_varint(&mut edit, 7);
        for (level, number, bytes) in tables {
            std::fs::write(dir.join(format!("{number:06}.ldb")), bytes).unwrap();
            put_varint(&mut edit, 7);
            put_varint(&mut edit, *level);
            put_varint(&mut edit, *number);
            put_varint(&mut edit, bytes.len() as u64);
            put_varint(&mut edit, 0);
            put_varint(&mut edit, 0);
        }
        std::fs::write(dir.join("MANIFEST-000009"), log_file(&[edit])).unwrap();
        std::fs::write(dir.join("CURRENT"), "MANIFEST-000009\n").unwrap();
        if let Some(log) = log {
            std::fs::write(dir.join("000007.log"), log).unwrap();
        }
        dir
    }

    pub fn everything(dir: &Path) -> (BTreeMap<Vec<u8>, Vec<u8>>, Damage) {
        let mut db = Db::open(dir).unwrap();
        let mut out = BTreeMap::new();
        db.for_each(
            &mut |k, v| {
                out.insert(k.to_vec(), v.to_vec());
            },
            &mut |_, _| true,
        );
        (out, db.damage.clone())
    }

    #[test]
    fn checksums_are_what_leveldb_computes() {
        // the published test vector of CRC-32C
        assert_eq!(crc32c(&[b"123456789"]), 0xe306_9283);
        assert_eq!(crc32c(&[b"1234", b"56789"]), 0xe306_9283);
    }

    #[test]
    fn the_newest_version_of_each_key_wins() {
        let old = table_file(
            &[
                (b"a", 1, true, b"old a".to_vec()),
                (b"b", 2, true, b"old b".to_vec()),
                (b"c", 3, true, b"old c".to_vec()),
                (b"d", 4, true, b"only in the old".to_vec()),
                (b"e", 5, true, b"old e".to_vec()),
            ],
            4,
        );
        let newer = table_file(
            &[
                // inside one table, the higher sequence is the newer
                (b"a", 10, true, b"new a".to_vec()),
                (b"a", 9, true, b"middle a".to_vec()),
                (b"b", 11, false, Vec::new()),
            ],
            0,
        );
        let log = log_file(&[batch(&[
            (b"c", Some(b"log c")),
            (b"e", None),
            (b"z", Some(b"log z")),
        ])]);
        // the deep table has the lower level, whatever its number
        let dir = db_folder("newest", &[(0, 20, newer), (1, 30, old)], Some(log));
        let (all, damage) = everything(&dir);
        let get = |k: &[u8]| all.get(k).map(Vec::as_slice);
        assert_eq!(get(b"a"), Some(&b"new a"[..]));
        assert_eq!(get(b"b"), None, "deleted in a newer table");
        assert_eq!(get(b"c"), Some(&b"log c"[..]));
        assert_eq!(get(b"d"), Some(&b"only in the old"[..]));
        assert_eq!(get(b"e"), None, "deleted in the log");
        assert_eq!(get(b"z"), Some(&b"log z"[..]));
        assert_eq!(damage, Damage::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_record_longer_than_a_log_block_comes_back_whole() {
        let big = vec![7u8; LOG_BLOCK * 2 + 100];
        let file = log_file(&[b"small".to_vec(), big.clone(), b"after".to_vec()]);
        assert_eq!(
            log_records(&file),
            vec![b"small".to_vec(), big, b"after".to_vec()]
        );
        // cut short: what came before stands
        assert_eq!(log_records(&file[..LOG_BLOCK]), vec![b"small".to_vec()]);
    }

    #[test]
    fn damage_is_counted_and_never_a_crash() {
        let good = table_file(&[(b"keep", 1, true, b"kept".to_vec())], 0);
        let mut bad = table_file(&[(b"lost", 2, true, b"gone".to_vec())], 0);
        bad[3] ^= 0xFF; // inside its data block: the checksum must catch it
        let dir = db_folder("damage", &[(1, 2, good), (1, 3, bad)], None);
        let (all, damage) = everything(&dir);
        assert!(all.contains_key(&b"keep"[..]));
        assert!(!all.contains_key(&b"lost"[..]));
        assert_eq!(damage.blocks, 1);

        std::fs::remove_file(dir.join("000003.ldb")).unwrap();
        assert_eq!(Db::open(&dir).unwrap().damage.missing_tables, 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn any_bytes_at_all_are_only_an_error() {
        let mut seed = 0x1234_5678u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let good = table_file(&[(b"key", 1, true, vec![1, 2, 3])], 4);
        for _ in 0..3000 {
            // random bytes, and a good table with some of its bytes changed
            let len = (next() % 200) as usize;
            let noise: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            let mut changed = good.clone();
            for _ in 0..1 + next() % 8 {
                let at = next() as usize % changed.len();
                changed[at] = next() as u8;
            }
            for file in [&noise, &changed] {
                let mut seen = HashSet::new();
                read_table(file, &mut seen, &mut |_, _| {}, &mut Damage::default());
                let _ = log_records(file);
                let mut written = BTreeMap::new();
                let _ = apply_batch(file, &mut written);
            }
        }
    }

    #[test]
    fn a_stop_is_a_stop() {
        let table = table_file(&[(b"a", 1, true, vec![1])], 0);
        let dir = db_folder("stop", &[(1, 2, table)], None);
        let mut db = Db::open(&dir).unwrap();
        let mut visited = 0;
        let finished = db.for_each(&mut |_, _| visited += 1, &mut |done, _| done == 0);
        assert!(!finished);
        assert_eq!(visited, 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
