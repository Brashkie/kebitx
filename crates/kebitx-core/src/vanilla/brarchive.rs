//! Reader for Minecraft Bedrock's `.brarchive` bundles.
//!
//! From 1.26.51 onward the game stopped shipping its textures, models, animations
//! and entities as loose files and packed them into `.brarchive` bundles. Any tool
//! that walked `resource_packs/vanilla/textures/` now sees an empty directory —
//! and, worse, fails silently: it finds zero vanilla assets and every addon that
//! reuses one starts looking "broken".
//!
//! The format, recovered by comparing offsets and sizes:
//!
//! ```text
//! 0x00  8 bytes   magic  7d 27 25 b1 a0 52 70 26
//! 0x08  u32       entry count
//! 0x0c  u32       version (always 1 so far)
//! 0x10  N x 256   entry table, each:
//!                     byte 0      name length
//!                     bytes 1..   the name
//!                     byte 248    u32 offset (from the start of the data block)
//!                     byte 252    u32 size
//! then            the data, back to back
//! ```
//!
//! Verified across 1841 bundles on a real install: none unreadable, and in every
//! one the last entry ends exactly where the file ends.
//!
//! Std-only, like the rest of the crate.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const MAGIC: [u8; 8] = [0x7d, 0x27, 0x25, 0xb1, 0xa0, 0x52, 0x70, 0x26];
const HEADER: usize = 0x10;
const ENTRY: usize = 0x100;
const OFF_OFFSET: usize = 248;
const OFF_SIZE: usize = 252;

pub struct Brarchive {
    data: Data,
    /// Total size of the bundle, in bytes.
    len: usize,
    data_start: usize,
    /// name -> (offset, size)
    pub entries: BTreeMap<String, (u32, u32)>,
}

/// Where the entries' contents are read from.
enum Data {
    Memory(Vec<u8>),
    /// Only the header and the entry table were read; contents are fetched
    /// on demand. The game's font bundle is 161 MB and nothing in it is ever
    /// needed: reading it whole was most of the catalog's time.
    File(PathBuf),
}

/// Little-endian u32 at `i`, or `None` if it does not fit in `b`.
fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    let s = b.get(i..i.checked_add(4)?)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

impl Brarchive {
    /// Open a bundle reading ONLY its header and entry table.
    pub fn open(path: &Path) -> Result<Self, String> {
        let at = |e: std::io::Error| format!("{}: {e}", path.display());
        let mut f = File::open(path).map_err(at)?;
        let len = usize::try_from(f.metadata().map_err(at)?.len())
            .map_err(|_| format!("{}: too large", path.display()))?;
        let mut head = [0u8; HEADER];
        if len < HEADER {
            return Err(format!("not a .brarchive: {}", path.display()));
        }
        f.read_exact(&mut head).map_err(at)?;
        let table_end =
            Self::table_end(&head, len).map_err(|why| format!("{why}: {}", path.display()))?;
        let mut table = vec![0u8; table_end];
        table[..HEADER].copy_from_slice(&head);
        f.read_exact(&mut table[HEADER..]).map_err(at)?;
        let entries =
            Self::parse_table(&table).map_err(|why| format!("{why}: {}", path.display()))?;
        Ok(Self {
            data: Data::File(path.to_path_buf()),
            len,
            data_start: table_end,
            entries,
        })
    }

    /// Parse a bundle already in memory. The bytes are untrusted: every count,
    /// offset and size read from them is checked, with overflow-checked
    /// arithmetic, before it is used to index anything. A malformed bundle is
    /// an `Err`, never a panic.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, &'static str> {
        let head = bytes.get(..HEADER).ok_or("not a .brarchive")?;
        let data_start = Self::table_end(head, bytes.len())?;
        let entries = Self::parse_table(&bytes[..data_start])?;
        Ok(Self {
            len: bytes.len(),
            data: Data::Memory(bytes),
            data_start,
            entries,
        })
    }

    /// Check the header and return where the entry table ends (= where the
    /// data starts), refusing a table that cannot fit in a file of `len` bytes.
    fn table_end(head: &[u8], len: usize) -> Result<usize, &'static str> {
        if head.len() < HEADER || head[..8] != MAGIC {
            return Err("not a .brarchive");
        }
        let n = u32_at(head, 8).ok_or("truncated header")? as usize;
        n.checked_mul(ENTRY)
            .and_then(|t| t.checked_add(HEADER))
            .filter(|&end| end <= len)
            .ok_or("truncated entry table")
    }

    /// `table` = header + the whole entry table, length checked by `table_end`.
    fn parse_table(table: &[u8]) -> Result<BTreeMap<String, (u32, u32)>, &'static str> {
        let n = (table.len() - HEADER) / ENTRY;
        let mut entries = BTreeMap::new();
        for i in 0..n {
            // in range: HEADER + n * ENTRY == table.len()
            let b = HEADER + i * ENTRY;
            let len = table[b] as usize; // < 256, so the name stays inside its entry
            let name = String::from_utf8_lossy(&table[b + 1..b + 1 + len]).to_string();
            let off = u32_at(table, b + OFF_OFFSET).ok_or("truncated entry")?;
            let size = u32_at(table, b + OFF_SIZE).ok_or("truncated entry")?;
            entries.insert(name, (off, size));
        }
        Ok(entries)
    }

    /// Where an entry's data lies, if it lies inside the file.
    fn span(&self, off: u32, size: u32) -> Option<(usize, usize)> {
        let a = self.data_start.checked_add(off as usize)?;
        let b = a.checked_add(size as usize)?;
        (b <= self.len).then_some((a, b))
    }

    /// One entry's contents.
    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        self.read_matching(|n| n == name)
            .into_iter()
            .next()
            .map(|(_, b)| b)
    }

    /// The contents of every entry whose name passes `wanted`, in name order,
    /// with the bundle opened once. Entries outside the file are skipped.
    pub fn read_matching(&self, wanted: impl Fn(&str) -> bool) -> Vec<(String, Vec<u8>)> {
        let picked: Vec<(&String, usize, usize)> = self
            .entries
            .iter()
            .filter(|(name, _)| wanted(name))
            .filter_map(|(name, &(off, size))| self.span(off, size).map(|(a, b)| (name, a, b)))
            // an entry claiming more than a text file may hold is not read:
            // a hostile table must not make us allocate gigabytes
            .filter(|(_, a, b)| (b - a) as u64 <= crate::filesystem::MAX_TEXT_BYTES)
            .collect();
        if picked.is_empty() {
            return Vec::new();
        }
        match &self.data {
            Data::Memory(bytes) => picked
                .into_iter()
                .map(|(n, a, b)| (n.clone(), bytes[a..b].to_vec()))
                .collect(),
            Data::File(path) => {
                let Ok(mut f) = File::open(path) else {
                    return Vec::new();
                };
                let mut out = Vec::with_capacity(picked.len());
                for (name, a, b) in picked {
                    let mut buf = vec![0u8; b - a];
                    if f.seek(SeekFrom::Start(a as u64)).is_ok() && f.read_exact(&mut buf).is_ok() {
                        out.push((name.clone(), buf));
                    }
                }
                out
            }
        }
    }

    /// Sanity check: every entry lies inside the file, and the last one ends
    /// exactly at end-of-file.
    pub fn is_consistent(&self) -> bool {
        let mut end = self.data_start;
        for &(off, size) in self.entries.values() {
            match self.span(off, size) {
                Some((_, b)) => end = end.max(b),
                None => return false,
            }
        }
        end == self.len
    }
}

/// Every `.brarchive` inside a pack, sorted.
pub fn bundles(pack_root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect(&pack_root.join("__brarchive"), &mut out);
    out.sort();
    out
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().map(|x| x == "brarchive").unwrap_or(false) {
            out.push(p);
        }
    }
}

/// Which pack-relative folder a bundle stands for.
///
///   `__brarchive/textures/blocks.brarchive` + `stone.png` -> `textures/blocks/stone.png`
///   `__brarchive/entity.brarchive`          + `cow.entity.json` -> `entity/cow.entity.json`
pub fn folder_of(pack_root: &Path, bundle: &Path) -> String {
    let root = pack_root.join("__brarchive");
    let rel = bundle
        .strip_prefix(&root)
        .unwrap_or(bundle)
        .to_string_lossy()
        .replace('\\', "/");
    rel.trim_end_matches(".brarchive").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle(entries: &[(&str, u32, u32)], data: &[u8]) -> Vec<u8> {
        let mut b = MAGIC.to_vec();
        b.extend((entries.len() as u32).to_le_bytes());
        b.extend([0u8; 4]);
        for (name, off, size) in entries {
            let mut e = [0u8; ENTRY];
            e[0] = name.len() as u8;
            e[1..1 + name.len()].copy_from_slice(name.as_bytes());
            e[OFF_OFFSET..OFF_OFFSET + 4].copy_from_slice(&off.to_le_bytes());
            e[OFF_SIZE..OFF_SIZE + 4].copy_from_slice(&size.to_le_bytes());
            b.extend(e);
        }
        b.extend(data);
        b
    }

    #[test]
    fn reads_a_well_formed_bundle() {
        let a =
            Brarchive::from_bytes(bundle(&[("a.json", 0, 2), ("b.json", 2, 3)], b"{}xyz")).unwrap();
        assert_eq!(a.read("a.json"), Some(b"{}".to_vec()));
        assert_eq!(a.read("b.json"), Some(b"xyz".to_vec()));
        assert!(a.is_consistent());
    }

    #[test]
    fn from_disk_only_the_table_is_read_and_contents_on_demand() {
        let dir = std::env::temp_dir().join(format!("kebitx-brarchive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.brarchive");
        std::fs::write(
            &path,
            bundle(
                &[("a.json", 0, 2), ("b.png", 2, 3), ("c.json", 5, 1)],
                b"{}xyz7",
            ),
        )
        .unwrap();

        let a = Brarchive::open(&path).unwrap();
        assert!(a.is_consistent());
        let json: Vec<(String, Vec<u8>)> = a.read_matching(|n| n.ends_with(".json"));
        assert_eq!(
            json,
            [
                ("a.json".to_string(), b"{}".to_vec()),
                ("c.json".to_string(), b"7".to_vec())
            ]
        );
        assert_eq!(a.read("b.png"), Some(b"xyz".to_vec()));

        // hostile files on disk: a huge entry count, a truncated header
        let mut huge = MAGIC.to_vec();
        huge.extend(u32::MAX.to_le_bytes());
        huge.extend([0u8; 4]);
        std::fs::write(&path, &huge).unwrap();
        assert!(Brarchive::open(&path).is_err());
        std::fs::write(&path, MAGIC).unwrap();
        assert!(Brarchive::open(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hostile_offsets_and_counts_are_errors_not_panics() {
        // entry pointing far past the end, and one whose offset + size overflows
        let a = Brarchive::from_bytes(bundle(
            &[("x", u32::MAX, u32::MAX), ("y", 1, u32::MAX)],
            b"ab",
        ))
        .unwrap();
        assert_eq!(a.read("x"), None);
        assert_eq!(a.read("y"), None);
        assert!(!a.is_consistent());

        // a count claiming billions of entries in a 16-byte file
        let mut huge = MAGIC.to_vec();
        huge.extend(u32::MAX.to_le_bytes());
        huge.extend([0u8; 4]);
        assert!(Brarchive::from_bytes(huge).is_err());

        // truncated header, wrong magic, empty
        assert!(Brarchive::from_bytes(MAGIC.to_vec()).is_err());
        assert!(Brarchive::from_bytes(vec![0u8; 32]).is_err());
        assert!(Brarchive::from_bytes(Vec::new()).is_err());
    }
}
