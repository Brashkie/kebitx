//! Reading ZIP archives (a Java `.jar`, a `.mcpack`): the central directory
//! is read once, an entry's data on demand. Stored and DEFLATE entries; each
//! one's CRC-32 is checked. No ZIP64, no encryption: refused, not guessed.

use super::{crc32, inflate};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
struct EntryInfo {
    method: u16,
    crc: u32,
    compressed: u64,
    size: u64,
    local_offset: u64,
}

/// Where the archive's bytes are: a file, opened once and read on demand
/// (opening it again for each entry cost more than reading: 11 383 opens for
/// a game `.jar`), or memory (a `.jar` inside a `.jar`, never written to
/// disk).
enum Backing {
    File(Mutex<File>),
    Memory(Arc<[u8]>),
}

pub struct ZipArchive {
    backing: Backing,
    entries: HashMap<String, EntryInfo>,
}

fn le16(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn le32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

impl ZipArchive {
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let len = f.metadata().map_err(|e| e.to_string())?.len();
        let entries = Self::directory(&mut f, len)?;
        Ok(Self {
            backing: Backing::File(Mutex::new(f)),
            entries,
        })
    }

    /// An archive held in memory: a `.jar` bundled inside another one.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        let data: Arc<[u8]> = bytes.into();
        let len = data.len() as u64;
        let entries = Self::directory(&mut Cursor::new(&data[..]), len)?;
        Ok(Self {
            backing: Backing::Memory(data),
            entries,
        })
    }

    fn directory<R: Read + Seek>(
        f: &mut R,
        len: u64,
    ) -> Result<HashMap<String, EntryInfo>, String> {
        // the end record is in the last 22 + 65535 bytes (comment included)
        let tail_len = len.min(22 + 65535);
        f.seek(SeekFrom::Start(len - tail_len))
            .map_err(|e| e.to_string())?;
        let mut tail = vec![0u8; tail_len as usize];
        f.read_exact(&mut tail).map_err(|e| e.to_string())?;
        let eocd = (0..tail.len().saturating_sub(21))
            .rev()
            .find(|&i| le32(&tail, i) == 0x0605_4b50)
            .ok_or("not a ZIP archive (no end record)")?;
        let count = le16(&tail, eocd + 10) as usize;
        let cd_size = le32(&tail, eocd + 12) as u64;
        let cd_offset = le32(&tail, eocd + 16) as u64;
        if cd_offset == 0xffff_ffff || count == 0xffff {
            return Err("ZIP64 archives are not supported".into());
        }
        if cd_offset.checked_add(cd_size).is_none_or(|e| e > len) {
            return Err("ZIP central directory out of the file".into());
        }
        f.seek(SeekFrom::Start(cd_offset))
            .map_err(|e| e.to_string())?;
        let mut cd = vec![0u8; cd_size as usize];
        f.read_exact(&mut cd).map_err(|e| e.to_string())?;
        let mut entries = HashMap::with_capacity(count);
        let mut i = 0;
        for _ in 0..count {
            let head = cd.get(i..i + 46).ok_or("ZIP central directory cut short")?;
            if le32(head, 0) != 0x0201_4b50 {
                return Err("bad ZIP central directory entry".into());
            }
            let flags = le16(head, 8);
            let (name_len, extra_len, comment_len) = (
                le16(head, 28) as usize,
                le16(head, 30) as usize,
                le16(head, 32) as usize,
            );
            let name = cd
                .get(i + 46..i + 46 + name_len)
                .ok_or("ZIP name cut short")?;
            let info = EntryInfo {
                method: le16(head, 10),
                crc: le32(head, 16),
                compressed: le32(head, 20) as u64,
                size: le32(head, 24) as u64,
                local_offset: le32(head, 42) as u64,
            };
            if flags & 1 == 0 {
                entries.insert(String::from_utf8_lossy(name).replace('\\', "/"), info);
            }
            i += 46 + name_len + extra_len + comment_len;
        }
        Ok(entries)
    }

    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.entries.keys()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// An entry's bytes, if it exists and is at most `limit` bytes.
    pub fn read(&self, name: &str, limit: u64) -> Result<Vec<u8>, String> {
        let e = self
            .entries
            .get(name)
            .ok_or_else(|| format!("{name}: not in the archive"))?;
        if e.size > limit {
            return Err(format!("{name}: larger than allowed"));
        }
        let packed = self.stored(name, e, limit)?;
        let data = match e.method {
            0 => packed,
            8 => {
                inflate::inflate(&packed, e.size as usize)
                    .map_err(|err| format!("{name}: {err}"))?
                    .0
            }
            m => return Err(format!("{name}: compression method {m} not supported")),
        };
        if data.len() as u64 != e.size || crc32(&data) != e.crc {
            return Err(format!("{name}: size or CRC mismatch"));
        }
        Ok(data)
    }

    /// The same archive, held in memory: for reading most of a large archive
    /// on every core, where one shared file handle would make them queue.
    pub fn in_memory(&self) -> Result<ZipArchive, String> {
        let data = match &self.backing {
            Backing::Memory(d) => d.clone(),
            Backing::File(file) => {
                let mut f = file
                    .lock()
                    .map_err(|_| "the archive's file lock is poisoned".to_string())?;
                f.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
                let mut all = Vec::new();
                f.read_to_end(&mut all).map_err(|e| e.to_string())?;
                all.into()
            }
        };
        Ok(ZipArchive {
            backing: Backing::Memory(data),
            entries: self.entries.clone(),
        })
    }

    /// An entry as stored: `(method, uncompressed size, CRC-32, bytes)`
    /// (method 8 is raw DEFLATE). For tools that decode it another way, and
    /// for measuring decoders on the same input.
    pub fn read_raw(&self, name: &str, limit: u64) -> Result<(u16, u64, u32, Vec<u8>), String> {
        let e = self
            .entries
            .get(name)
            .ok_or_else(|| format!("{name}: not in the archive"))?;
        if e.size > limit {
            return Err(format!("{name}: larger than allowed"));
        }
        Ok((e.method, e.size, e.crc, self.stored(name, e, limit)?))
    }

    /// The stored (compressed) bytes of an entry. A file is opened once per
    /// archive and shared: only the seek and read happen under its lock,
    /// decoding happens outside, on every core.
    fn stored(&self, name: &str, e: &EntryInfo, limit: u64) -> Result<Vec<u8>, String> {
        fn take<R: Read + Seek>(
            f: &mut R,
            name: &str,
            e: &EntryInfo,
            limit: u64,
        ) -> Result<Vec<u8>, String> {
            f.seek(SeekFrom::Start(e.local_offset))
                .map_err(|e| e.to_string())?;
            let mut head = [0u8; 30];
            f.read_exact(&mut head).map_err(|e| e.to_string())?;
            if le32(&head, 0) != 0x0403_4b50 {
                return Err(format!("{name}: bad local header"));
            }
            let skip = le16(&head, 26) as i64 + le16(&head, 28) as i64;
            f.seek(SeekFrom::Current(skip)).map_err(|e| e.to_string())?;
            let mut packed = vec![
                0u8;
                e.compressed
                    .min(limit.saturating_mul(1032).saturating_add(1024))
                    as usize
            ];
            f.read_exact(&mut packed)
                .map_err(|err| format!("{name}: {err}"))?;
            Ok(packed)
        }
        match &self.backing {
            Backing::File(file) => {
                let mut f = file
                    .lock()
                    .map_err(|_| format!("{name}: the archive's file lock is poisoned"))?;
                take(&mut *f, name, e, limit)
            }
            Backing::Memory(d) => take(&mut Cursor::new(&d[..]), name, e, limit),
        }
    }
}
